#include "MarkdownHtml.h"
#include "MonospaceFont.h"

#include <QFontDatabase>
#include <QFontMetricsF>
#include <QGuiApplication>
#include <QObject>
#include <QRegularExpression>
#include <QStringList>
#include <QVector>

#include <md4c.h>
extern "C" {
#include <entity.h>
}

namespace {
// github-markdown-css 5.8.1 comment metrics at a 14 px base font (rem = 16 px).
constexpr int BaseSize = 14, CodeSize = 12;
constexpr int BlockGap = 16, HeadingGap = 24, RuleGap = 24;
constexpr int LineHeight = 21, CodeLineHeight = 23;
// Line heights are fixed (-qt-line-height-type): Qt takes a plain px value as a
// minimum and aligns each line to the bottom of its box, so an inline-code chip
// reaching below the text would lift or grow its line. Fixed lines keep one pitch
// and let the chip padding overflow, like GitHub's line boxes.
constexpr int Gutter = 28;   // list padding-left: 2em

QColor over(const QColor &top, double alpha, const QColor &bottom)
{
    const auto mix = [alpha](float a, float b) { return a * alpha + b * (1 - alpha); };
    return QColor::fromRgbF(mix(top.redF(), bottom.redF()), mix(top.greenF(), bottom.greenF()), mix(top.blueF(), bottom.blueF()));
}

QString codepoint(uint value)
{
    if (value == 0 || value > 0x10FFFF || (value >= 0xD800 && value <= 0xDFFF)) return QString(QChar(QChar::ReplacementCharacter));
    const char32_t character = value;
    return QString::fromUcs4(&character, 1);
}

// "&amp;", "&#65;" or "&#x42;" as reported by md4c (MD_TEXT_ENTITY).
QString decodeEntity(const QByteArray &entity)
{
    if (entity.startsWith("&#")) {
        const bool hex = entity.size() > 3 && (entity[2] == 'x' || entity[2] == 'X');
        bool ok = false;
        const uint value = entity.mid(hex ? 3 : 2, entity.size() - (hex ? 4 : 3)).toUInt(&ok, hex ? 16 : 10);
        return ok ? codepoint(value) : QString(QChar(QChar::ReplacementCharacter));
    }
    const ENTITY *found = entity_lookup(entity.constData(), size_t(entity.size()));
    if (!found) return QString::fromUtf8(entity);
    return codepoint(found->codepoints[0]) + (found->codepoints[1] ? codepoint(found->codepoints[1]) : QString());
}

QString attribute(const MD_ATTRIBUTE &value)
{
    QString result;
    for (int i = 0; value.substr_offsets[i] < value.size; ++i) {
        const QByteArray part(value.text + value.substr_offsets[i], qsizetype(value.substr_offsets[i + 1] - value.substr_offsets[i]));
        if (value.substr_types[i] == MD_TEXT_NULLCHAR) result += QChar(QChar::ReplacementCharacter);
        else if (value.substr_types[i] == MD_TEXT_ENTITY) result += decodeEntity(part);
        else result += QString::fromUtf8(part);
    }
    return result;
}

int headingSize(unsigned level)
{
    static constexpr int sizes[] = {28, 21, 18, 14, 12, 12};
    return sizes[qBound(1u, level, 6u) - 1];
}

// Inline formatting is written on every text run: Qt drops inherited font
// properties on nested spans inside table cells.
struct Style {
    int size = BaseSize;
    bool bold = false, italic = false, strike = false;
    QColor color;
    QString family;
    QString css() const
    {
        QString result = QString("font-size:%1px;").arg(size);
        if (bold) result += "font-weight:600;";
        if (italic) result += "font-style:italic;";
        if (strike) result += "text-decoration:line-through;";
        if (color.isValid()) result += QString("color:%1;").arg(color.name());
        if (!family.isEmpty()) result += QString("font-family:'%1';").arg(family);
        return result;
    }
};

struct Frame {
    MD_BLOCKTYPE type = MD_BLOCK_DOC;
    QString html, raw;
    int blocks = 0;          // child blocks (or list rows) emitted so far
    bool afterRule = false;  // the previous child was a thematic break
    bool code = false;       // contains inline code
    bool ordered = false, loose = false, task = false, checked = false, head = false;
    unsigned number = 1, level = 0;
    int depth = 0, bodyRow = 0;
    MD_ALIGN align = MD_ALIGN_DEFAULT;
};

struct Link {
    qsizetype start = 0;
    QString label, destination;
    MarkdownLink target;
};

class Renderer {
public:
    Renderer(const MarkdownTheme &theme, const MarkdownLinkPolicy &links) : m_theme(theme), m_links(links)
    {
        m_frames.append(Frame{});
        Style base; base.color = theme.fg; m_styles.append(base);
    }
    QString html() const { return m_frames.first().html; }

    int enterBlock(MD_BLOCKTYPE type, void *detail)
    {
        if (type == MD_BLOCK_DOC) return 0;
        Frame frame; frame.type = type;
        switch (type) {
        case MD_BLOCK_H: {
            frame.level = static_cast<MD_BLOCK_H_DETAIL *>(detail)->level;
            const int size = headingSize(frame.level); const bool muted = frame.level == 6;
            pushStyle([&](Style &s) { s.size = size; s.bold = true; if (muted) s.color = m_theme.muted; });
            break;
        }
        case MD_BLOCK_UL:
            frame.loose = !static_cast<MD_BLOCK_UL_DETAIL *>(detail)->is_tight;
            frame.depth = listDepth();
            break;
        case MD_BLOCK_OL: {
            const auto *list = static_cast<MD_BLOCK_OL_DETAIL *>(detail);
            frame.ordered = true; frame.loose = !list->is_tight; frame.number = list->start; frame.depth = listDepth();
            break;
        }
        case MD_BLOCK_LI: {
            const auto *item = static_cast<MD_BLOCK_LI_DETAIL *>(detail);
            frame.task = item->is_task;
            frame.checked = item->is_task && (item->task_mark == 'x' || item->task_mark == 'X');
            break;
        }
        case MD_BLOCK_QUOTE:
            pushStyle([&](Style &s) { s.color = m_theme.muted; });
            break;
        case MD_BLOCK_TH:
            pushStyle([](Style &s) { s.bold = true; });
            frame.align = static_cast<MD_BLOCK_TD_DETAIL *>(detail)->align;
            break;
        case MD_BLOCK_TD:
            frame.align = static_cast<MD_BLOCK_TD_DETAIL *>(detail)->align;
            break;
        case MD_BLOCK_TR:
            frame.head = m_frames.last().type == MD_BLOCK_THEAD;
            break;
        default: break;
        }
        m_frames.append(frame);
        return 0;
    }

    int leaveBlock(MD_BLOCKTYPE type, void *)
    {
        if (type == MD_BLOCK_DOC) return 0;
        const Frame frame = m_frames.takeLast();
        switch (type) {
        case MD_BLOCK_P:
            append(QString("<p style=\"margin:%1px 0 0 0;line-height:%2px;-qt-line-height-type:fixed;\">").arg(gap(BlockGap)).arg(frame.code ? CodeLineHeight : LineHeight)
                   + frame.html + "</p>");
            break;
        case MD_BLOCK_H: {
            m_styles.removeLast();
            const int size = headingSize(frame.level), lineHeight = qRound(size * 1.25), top = gap(HeadingGap);
            if (frame.level <= 2)
                append(QString("<table width=\"100%\" cellspacing=\"0\" cellpadding=\"0\" style=\"margin-top:%1px;border-collapse:collapse;\">"
                               "<tr><td style=\"padding-bottom:%2px;border-bottom:1px solid %3;line-height:%4px;-qt-line-height-type:fixed;\">")
                           .arg(top).arg(qRound(size * 0.3)).arg(m_theme.borderMuted.name()).arg(lineHeight)
                       + frame.html + "</td></tr></table>");
            else append(QString("<p style=\"margin:%1px 0 0 0;line-height:%2px;-qt-line-height-type:fixed;\">").arg(top).arg(lineHeight) + frame.html + "</p>");
            break;
        }
        case MD_BLOCK_UL: case MD_BLOCK_OL: {
            // A nested list continues its item without a gap (GitHub: ul ul { margin: 0 }).
            const int top = m_frames.last().type == MD_BLOCK_LI ? 0 : gap(BlockGap);
            append(QString("<table cellspacing=\"0\" cellpadding=\"0\" style=\"margin-top:%1px;\">").arg(top) + frame.html + "</table>");
            break;
        }
        case MD_BLOCK_LI: {
            Frame &list = m_frames.last();
            const int top = list.blocks == 0 ? 0 : list.loose ? BlockGap : 3;
            const int lineHeight = frame.code ? CodeLineHeight : LineHeight;
            // The marker hangs in the gutter on the first line itself. In a cell of its
            // own Qt would put it on another baseline than a monospace or chip first line.
            QString marker;
            if (frame.task) marker = image(frame.checked ? "check-on" : "check-off", Gutter, 18, true);
            else if (list.ordered) {
                QFont font = QGuiApplication::font(); font.setPixelSize(BaseSize);
                const QString label = ordinal(list.number, list.depth) + '.';
                const int width = qRound(QFontMetricsF(font).horizontalAdvance(label));
                marker = image("blank", qMax(1, Gutter - 4 - width), 1) + run(label) + image("gap", 4, 1);
            } else marker = image(list.depth == 0 ? "disc" : list.depth == 1 ? "circle" : "square", Gutter, 12);
            const QString indent = QString("text-indent:-%1px;").arg(Gutter);
            QString content = frame.html;
            if (content.startsWith(QLatin1String("<p style=\""))) {
                const auto end = content.indexOf(QLatin1String("\">"));
                content.insert(end + 2, marker);
                content.insert(end, indent);
            } else {
                // Tight items hold inline runs, then any blocks (nested lists, code).
                const auto block = content.indexOf(QLatin1String("<table"));
                content = QString("<p style=\"margin:0px 0 0 0;line-height:%1px;-qt-line-height-type:fixed;%2\">").arg(lineHeight).arg(indent)
                    + marker + (block < 0 ? content : content.left(block)) + "</p>" + (block < 0 ? QString() : content.mid(block));
            }
            // No valign: Qt would hand it to the marker image (AlignTop) and lift it off the baseline.
            list.html += QString("<tr><td style=\"padding:%1px 0 0 %2px;\">").arg(top).arg(Gutter) + content + "</td></tr>";
            ++list.blocks; ++list.number;
            break;
        }
        case MD_BLOCK_QUOTE:
            m_styles.removeLast();
            append(QString("<table width=\"100%\" cellspacing=\"0\" cellpadding=\"0\" style=\"margin-top:%1px;\"><tr>"
                           "<td width=\"4\" bgcolor=\"%2\"></td><td style=\"padding:0 14px;\">").arg(gap(BlockGap)).arg(m_theme.border.name())
                   + frame.html + "</td></tr></table>");
            break;
        case MD_BLOCK_CODE:
            append(codeBlock(frame.raw, gap(BlockGap)));
            break;
        case MD_BLOCK_HR:
            append(QString("<table width=\"100%\" cellspacing=\"0\" cellpadding=\"0\" bgcolor=\"%1\" style=\"margin-top:%2px;\">"
                           "<tr><td height=\"4\" style=\"font-size:1px;line-height:4px;\">&#8203;</td></tr></table>")
                       .arg(m_theme.border.name()).arg(gap(RuleGap)), true);
            break;
        case MD_BLOCK_TABLE:
            append(QString("<table cellspacing=\"0\" cellpadding=\"0\" style=\"margin-top:%1px;border-collapse:collapse;\">").arg(gap(BlockGap))
                   + frame.html + "</table>");
            break;
        case MD_BLOCK_THEAD: case MD_BLOCK_TBODY:
            m_frames.last().html += frame.html;
            break;
        case MD_BLOCK_TR: {
            Frame *table = nullptr;
            for (auto i = m_frames.size() - 1; i >= 0 && !table; --i) if (m_frames[i].type == MD_BLOCK_TABLE) table = &m_frames[i];
            const bool zebra = !frame.head && table && table->bodyRow++ % 2 == 1;
            m_frames.last().html += QString("<tr bgcolor=\"%1\">").arg((zebra ? m_theme.stripe : m_theme.canvas).name()) + frame.html + "</tr>";
            break;
        }
        case MD_BLOCK_TH: case MD_BLOCK_TD: {
            const bool header = type == MD_BLOCK_TH;
            if (header) m_styles.removeLast();
            const QString tag = header ? QStringLiteral("th") : QStringLiteral("td");
            // Browsers centre header cells unless the column says otherwise.
            const QString align = frame.align == MD_ALIGN_LEFT ? "left" : frame.align == MD_ALIGN_CENTER ? "center"
                : frame.align == MD_ALIGN_RIGHT ? "right" : header ? "center" : "";
            m_frames.last().html += "<" + tag + (align.isEmpty() ? QString() : " align=\"" + align + "\"")
                + QString(" style=\"padding:6px 13px;border:1px solid %1;line-height:%2px;-qt-line-height-type:fixed;\">").arg(m_theme.border.name())
                      .arg(frame.code ? CodeLineHeight : LineHeight)
                + frame.html + "</" + tag + ">";
            break;
        }
        default:
            // Blocks without their own presentation keep their content.
            m_frames.last().html += frame.html;
            break;
        }
        return 0;
    }

    int enterSpan(MD_SPANTYPE type, void *detail)
    {
        switch (type) {
        case MD_SPAN_EM: pushStyle([](Style &s) { s.italic = true; }); break;
        case MD_SPAN_STRONG: pushStyle([](Style &s) { s.bold = true; }); break;
        case MD_SPAN_DEL: pushStyle([](Style &s) { s.strike = true; }); break;
        case MD_SPAN_CODE:
            m_inCode = true; m_frames.last().code = true;
            m_codeStart = m_frames.last().html.size(); m_codeText.clear();
            m_frames.last().html += codeOpening(m_styles.last());
            break;
        case MD_SPAN_A: {
            Link link; link.start = m_frames.last().html.size();
            link.destination = attribute(static_cast<MD_SPAN_A_DETAIL *>(detail)->href);
            if (m_links) link.target = m_links(link.destination);
            const bool active = !link.target.href.isEmpty();
            m_linkStack.append(link);
            pushStyle([&](Style &s) { if (active) s.color = m_theme.accent; });
            break;
        }
        case MD_SPAN_IMG:
            // WhiteWh fork: the policy sees the picture's source, so that Activity can
            // show a picture of the session's computer under the message. Nothing
            // here loads it.
            if (m_links) m_links(attribute(static_cast<MD_SPAN_IMG_DETAIL *>(detail)->src));
            if (!m_hidden) m_frames.last().html += run(QObject::tr("[Image attachment]"));
            ++m_hidden;
            break;
        default: break;
        }
        return 0;
    }

    int leaveSpan(MD_SPANTYPE type)
    {
        switch (type) {
        case MD_SPAN_EM: case MD_SPAN_STRONG: case MD_SPAN_DEL: m_styles.removeLast(); break;
        case MD_SPAN_CODE: {
            m_inCode = false;
            QString &html = m_frames.last().html;
            const MarkdownLink target = m_linkStack.isEmpty() ? urlTarget(m_codeText) : MarkdownLink{};
            if (target.href.isEmpty()) { html += "</span>"; break; }
            // An address quoted as code links like [`url`](url).
            Style style = m_styles.last(); style.color = m_theme.accent;
            html.truncate(m_codeStart);
            html += anchor(target) + codeOpening(style) + codeText(m_codeText) + "</span></a>";
            break;
        }
        case MD_SPAN_A: {
            m_styles.removeLast();
            const Link link = m_linkStack.takeLast();
            if (link.target.href.isEmpty()) break;
            QString &html = m_frames.last().html;
            const QString &location = link.target.location;
            if (!location.isEmpty() && link.label != location && link.label != link.destination) {
                Style style = m_styles.last(); style.color = m_theme.accent;
                html += QString("<span style=\"%1\">").arg(style.css()) + (" (" + location + ")").toHtmlEscaped() + "</span>";
            }
            html.insert(link.start, anchor(link.target));
            html += "</a>";
            break;
        }
        case MD_SPAN_IMG: --m_hidden; break;
        default: break;
        }
        return 0;
    }

    int text(MD_TEXTTYPE type, const MD_CHAR *data, MD_SIZE size)
    {
        if (m_hidden) return 0;
        Frame &frame = m_frames.last();
        if (type == MD_TEXT_BR) { frame.html += m_inCode ? QStringLiteral("&nbsp;") : QStringLiteral("<br>"); return 0; }
        QString value;
        if (type == MD_TEXT_NULLCHAR) value = QChar(QChar::ReplacementCharacter);
        else if (type == MD_TEXT_SOFTBR) value = QStringLiteral(" ");
        else if (type == MD_TEXT_ENTITY) value = decodeEntity(QByteArray(data, qsizetype(size)));
        else value = QString::fromUtf8(data, qsizetype(size));
        if (!m_linkStack.isEmpty()) m_linkStack.last().label += value;
        if (frame.type == MD_BLOCK_CODE) { frame.raw += value; return 0; }
        if (m_inCode) { frame.html += codeText(value); m_codeText += value; }
        else frame.html += type == MD_TEXT_NORMAL && m_linkStack.isEmpty() ? autolinked(value) : run(value);
        return 0;
    }

private:
    void pushStyle(const std::function<void(Style &)> &change) { Style style = m_styles.last(); change(style); m_styles.append(style); }
    // The first block of a container has no gap; a block after a rule keeps the rule's gap.
    int gap(int wanted) const { const Frame &c = m_frames.last(); return c.blocks == 0 ? 0 : c.afterRule ? RuleGap : wanted; }
    void append(const QString &html, bool rule = false) { Frame &c = m_frames.last(); c.html += html; ++c.blocks; c.afterRule = rule; }
    QString run(const QString &value, const Style &style) const { return QString("<span style=\"%1\">").arg(style.css()) + value.toHtmlEscaped() + "</span>"; }
    QString run(const QString &value) const { return run(value, m_styles.last()); }
    QString anchor(const MarkdownLink &target) const
    {
        return QString("<a href=\"%1\" title=\"%2\" style=\"color:%3;text-decoration:none;\">")
            .arg(target.href.toHtmlEscaped(), target.tooltip.toHtmlEscaped(), m_theme.accent.name());
    }
    // GitHub: code is 85 % of the surrounding text and inherits its colour, weight
    // and slant; in headings it keeps the heading size (h1 code { font-size: inherit }).
    QString codeOpening(const Style &around) const
    {
        const bool heading = m_frames.last().type == MD_BLOCK_H;
        return QString("<span style=\"background-color:%1;font-size:%2px;color:%3;%4%5\">")
            .arg((heading ? MarkdownHtml::headingChipSentinel() : MarkdownHtml::chipSentinel()).name())
            .arg(heading ? around.size : qRound(around.size * 0.85)).arg(around.color.name(),
                 around.bold ? QStringLiteral("font-weight:600;") : QString(),
                 around.italic ? QStringLiteral("font-style:italic;") : QString());
    }
    static QString codeText(const QString &value) { return value.toHtmlEscaped().replace(' ', QStringLiteral("&nbsp;")); }
    // The link for a text that is exactly one bare http(s):// or www. address.
    MarkdownLink urlTarget(const QString &text) const
    {
        static const QRegularExpression prefix(QStringLiteral("^(?:https?://|www\\.)"), QRegularExpression::CaseInsensitiveOption);
        const auto match = prefix.match(text);
        if (!m_links || !match.hasMatch()) return {};
        const bool www = match.captured().endsWith('.');
        if (www && (text.size() == 4 || !text[4].isLetterOrNumber())) return {};
        if (autolinkEnd(text, www ? 0 : match.capturedEnd()) != text.size()) return {};
        return m_links(www ? "http://" + text : text);
    }
    // GitHub's extended autolinks for bare http(s):// and www. addresses. md4c 0.5
    // drops a whole URL whose host has no dot or has a port, or whose path is not ASCII.
    QString autolinked(const QString &text) const
    {
        if (!m_links || (!text.contains(QLatin1String("://")) && !text.contains(QLatin1String("www."), Qt::CaseInsensitive))) return run(text);
        static const QRegularExpression start(QStringLiteral("(?<![\\p{L}\\p{N}_./@:-])(?:https?://|www\\.)"),
            QRegularExpression::CaseInsensitiveOption);
        QString result; qsizetype done = 0;
        for (auto it = start.globalMatch(text); it.hasNext();) {
            const auto match = it.next();
            if (match.capturedStart() < done) continue;
            const qsizetype end = autolinkEnd(text, match.captured().endsWith('.') ? match.capturedStart() : match.capturedEnd());
            if (end < 0) continue;
            const QString url = text.mid(match.capturedStart(), end - match.capturedStart());
            const MarkdownLink target = urlTarget(url);
            if (target.href.isEmpty()) continue;
            Style style = m_styles.last(); style.color = m_theme.accent;
            if (match.capturedStart() > done) result += run(text.mid(done, match.capturedStart() - done));
            result += anchor(target) + run(url, style) + "</a>";
            done = end;
        }
        if (done == 0) return run(text);
        return done < text.size() ? result + run(text.mid(done)) : result;
    }
    // The end of a link whose host starts at `host`, or -1. The host has no underscore
    // in its last two labels. The link runs to whitespace or "<", without trailing
    // punctuation, closing quotes or unbalanced ")".
    static qsizetype autolinkEnd(const QString &text, qsizetype host)
    {
        qsizetype end = host;
        while (end < text.size() && (text[end].isLetterOrNumber() || QStringLiteral("-_.").contains(text[end]))) ++end;
        if (end == host || !text[host].isLetterOrNumber()) return -1;
        const auto labels = text.mid(host, end - host).split('.');
        for (qsizetype i = qMax<qsizetype>(0, labels.size() - 2); i < labels.size(); ++i)
            if (labels[i].contains('_')) return -1;
        while (end < text.size() && !text[end].isSpace() && text[end] != '<') ++end;
        int open = 0, close = 0;
        for (qsizetype i = host; i < end; ++i) { open += text[i] == '('; close += text[i] == ')'; }
        while (end > host) {
            const QChar last = text[end - 1];
            if (last == ')' && close > open) { --close; --end; }
            else if (QStringLiteral("?!.,:;*\"'_~…").contains(last) || last.category() == QChar::Punctuation_FinalQuote) --end;
            else break;
        }
        return end;
    }
    int listDepth() const
    {
        int depth = 0;
        for (const auto &frame : m_frames) depth += frame.type == MD_BLOCK_UL || frame.type == MD_BLOCK_OL;
        return depth;
    }
    // Bullets stand on the baseline: their 12 px image puts the dot 5.5 px above it, as on GitHub.
    QString image(const char *name, int width, int height, bool middle = false) const
    {
        return QString("<img src=\"%1\" width=\"%2\" height=\"%3\"%4>").arg(m_theme.resource(QLatin1String(name))).arg(width).arg(height)
            .arg(middle ? QStringLiteral(" style=\"vertical-align:middle;\"") : QString());
    }
    // Runs, indentation and trailing spaces survive HTML whitespace collapsing;
    // a single space between words stays breakable for narrow panes.
    static QString preserveSpaces(const QString &escaped)
    {
        QString result;
        for (qsizetype i = 0; i < escaped.size(); ++i) {
            const bool space = escaped[i] == ' ';
            const bool lone = space && i > 0 && escaped[i - 1] != ' ' && i + 1 < escaped.size() && escaped[i + 1] != ' ';
            result += space && !lone ? QStringLiteral("&nbsp;") : QString(escaped[i]);
        }
        return result;
    }
    // Rounded corners: a 3×3 table with drawn 6 px corners around the padded code.
    QString codeBlock(QString code, int top) const
    {
        code.remove('\r'); code.replace('\t', QStringLiteral("    "));
        if (code.endsWith('\n')) code.chop(1);
        Style style; style.size = CodeSize; style.color = m_theme.fg; style.family = m_theme.monoFamily;
        QStringList lines;
        for (const QString &line : code.split('\n'))
            lines.append(QString("<span style=\"%1\">").arg(style.css()) + preserveSpaces(line.toHtmlEscaped()) + "</span>");
        const QString subtle = m_theme.subtle.name();
        const auto corner = [&](const char *name) {
            return QString("<td width=\"6\" height=\"6\" style=\"font-size:1px;line-height:6px;\"><img src=\"%1\" width=\"6\" height=\"6\"></td>")
                .arg(m_theme.resource(QLatin1String(name)));
        };
        const QString edge = QString("<td bgcolor=\"%1\" style=\"font-size:1px;line-height:6px;\">&#8203;</td>").arg(subtle);
        return QString("<table width=\"100%\" cellspacing=\"0\" cellpadding=\"0\" style=\"margin-top:%1px;\">").arg(top)
            + "<tr>" + corner("corner-tl") + edge + corner("corner-tr") + "</tr>"
            + QString("<tr><td bgcolor=\"%1\"></td><td bgcolor=\"%1\" style=\"padding:7px 10px 13px 10px;line-height:17px;\">").arg(subtle)
            + lines.join(QStringLiteral("<br>"))
            + QString("</td><td bgcolor=\"%1\"></td></tr>").arg(subtle)
            + "<tr>" + corner("corner-bl") + edge + corner("corner-br") + "</tr></table>";
    }
    // GitHub: ol → decimal, nested → lower-roman, deeper → lower-alpha.
    static QString ordinal(unsigned number, int depth)
    {
        if (depth == 0) return QString::number(number);
        if (depth == 1) {
            static const QList<QPair<unsigned, const char *>> numerals{{1000, "m"}, {900, "cm"}, {500, "d"}, {400, "cd"}, {100, "c"},
                {90, "xc"}, {50, "l"}, {40, "xl"}, {10, "x"}, {9, "ix"}, {5, "v"}, {4, "iv"}, {1, "i"}};
            QString result;
            for (const auto &[value, text] : numerals) while (number >= value) { result += QLatin1String(text); number -= value; }
            return result;
        }
        QString result;
        while (number > 0) { --number; result.prepend(QChar('a' + int(number % 26))); number /= 26; }
        return result;
    }

    const MarkdownTheme &m_theme;
    const MarkdownLinkPolicy &m_links;
    QVector<Frame> m_frames;
    QVector<Style> m_styles;
    QVector<Link> m_linkStack;
    int m_hidden = 0;   // inside image alt text
    bool m_inCode = false;
    qsizetype m_codeStart = 0;   // the open code span's markup in the current frame
    QString m_codeText;
};
}

MarkdownTheme MarkdownTheme::github(bool dark, const QColor &canvas, double scale)
{
    MarkdownTheme theme;
    theme.dark = dark; theme.scalePercent = qRound(scale * 100); theme.canvas = canvas;
    theme.fg = QColor(dark ? "#f0f6fc" : "#1f2328");
    theme.muted = QColor(dark ? "#9198a1" : "#59636e");
    theme.accent = QColor(dark ? "#4493f8" : "#0969da");
    theme.subtle = QColor(dark ? "#151b23" : "#f6f8fa");
    theme.stripe = theme.subtle;
    theme.border = QColor(dark ? "#3d444d" : "#d1d9e0");
    theme.borderMuted = over(theme.border, 0.7, canvas);
    theme.chip = QColor(dark ? "#656c76" : "#818b98"); theme.chip.setAlphaF(dark ? 0.2f : 0.12f);
    theme.monoFamily = MarkdownHtml::monospaceFamily();
    return theme;
}

QString MarkdownTheme::resource(const QString &name) const
{
    QString url = QString("hgs-md:%1/%2/%3").arg(name, dark ? QStringLiteral("dark") : QStringLiteral("light"), QString::number(scalePercent));
    // Code blocks may sit on the embedding view's own surface; their corners follow it.
    if (name.startsWith(QLatin1String("corner-"))) url += "?fill=" + subtle.name().mid(1);
    return url;
}

QColor MarkdownHtml::chipSentinel() { return QColor(1, 2, 3); }
QColor MarkdownHtml::headingChipSentinel() { return QColor(1, 2, 4); }

QString MarkdownHtml::monospaceFamily() { return ::monospaceFamily(); }

QString MarkdownHtml::render(const QString &markdown, const MarkdownTheme &theme, const MarkdownLinkPolicy &links)
{
    Renderer renderer(theme, links);
    MD_PARSER parser{};
    parser.abi_version = 0;
    // GitHub semantics: `_x_` is emphasis, so MD_FLAG_UNDERLINE stays off. Raw HTML remains text.
    // Bare URLs follow GitHub in Renderer::autolinked(); md4c still links e-mail addresses.
    parser.flags = (MD_DIALECT_GITHUB & ~(MD_FLAG_PERMISSIVEURLAUTOLINKS | MD_FLAG_PERMISSIVEWWWAUTOLINKS)) | MD_FLAG_NOHTML;
    parser.enter_block = [](MD_BLOCKTYPE type, void *detail, void *self) { return static_cast<Renderer *>(self)->enterBlock(type, detail); };
    parser.leave_block = [](MD_BLOCKTYPE type, void *detail, void *self) { return static_cast<Renderer *>(self)->leaveBlock(type, detail); };
    parser.enter_span = [](MD_SPANTYPE type, void *detail, void *self) { return static_cast<Renderer *>(self)->enterSpan(type, detail); };
    parser.leave_span = [](MD_SPANTYPE type, void *, void *self) { return static_cast<Renderer *>(self)->leaveSpan(type); };
    parser.text = [](MD_TEXTTYPE type, const MD_CHAR *text, MD_SIZE size, void *self) { return static_cast<Renderer *>(self)->text(type, text, size); };
    const QByteArray source = markdown.toUtf8();
    if (md_parse(source.constData(), MD_SIZE(source.size()), &parser, &renderer) != 0)
        return "<p>" + markdown.toHtmlEscaped() + "</p>";
    return renderer.html();
}
