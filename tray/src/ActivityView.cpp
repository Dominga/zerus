#include "ActivityView.h"
#include "PathProbe.h"
#include <QBuffer>
#include "MediaViewer.h"
#include "IronRules.h"
#include "ActivityWidth.h"
#include "ContentScale.h"
#include "MarkdownHtml.h"
#include "MarkdownObjects.h"
#include "ProcessSettings.h"
#include "QuestionReply.h"
#include "SessionFileReference.h"
#include "WorkspaceIcons.h"
#include "speech/VoicePlayerImage.h"
#include "speech/RoundVideo.h"
#include <QDialog>
#include <QVBoxLayout>

#include <QDateTime>
#include <QAudioOutput>
#include <QMediaPlayer>
#include <QAbstractTextDocumentLayout>
#include <QAction>
#include <QClipboard>
#include <QContextMenuEvent>
#include <QCryptographicHash>
#include <QCursor>
#include <QDesktopServices>
#include <QEvent>
#include <QEventLoop>
#include <QElapsedTimer>
#include <QGuiApplication>
#include <QHelpEvent>
#include <QPainter>
#include <QPainterPath>
#include <QTextTable>
#include <QtMath>
#include <QHideEvent>
#include <QShowEvent>
#include <QImage>
#include <QJsonDocument>
#include <QLabel>
#include <QMenu>
#include <QMimeData>
#include <QMouseEvent>
#include <QProgressBar>
#include <QHBoxLayout>
#include <QPushButton>
#include <QScrollBar>
#include <QSemaphore>
#include <QStyle>
#include <QStyleOption>
#include <QTextBlock>
#include <QTextBrowser>
#include <QTextCursor>
#include <QTextDocument>
#include <QTextFragment>
#include <QTextDocumentFragment>
#include <QTextFrame>
#include <QTextInlineObject>
#include <QTextObjectInterface>
#include <QFontMetrics>
#include <QTimer>
#include <QThread>
#include <QPointer>
#include <memory>
#include <QToolTip>
#include <QUrl>
#include <QUrlQuery>
#include <QVBoxLayout>
#include <QWheelEvent>

#include <algorithm>
#include <cmath>
#include <functional>
#include <utility>
#include <QRegularExpression>   // WhiteWh fork: agent team plaques
#include <tuple>

namespace ActivityMedia {
// A media preview: 320 x 180 in the document, drawn at twice that.
constexpr int Width = 320, Height = 180;
QImage tile(const QImage &content, const QString &name, bool play, const QString &note)
{
    QImage tile(Width * 2, Height * 2, QImage::Format_ARGB32_Premultiplied);
    tile.fill(Qt::transparent);
    QPainter p(&tile);
    p.setRenderHints(QPainter::Antialiasing | QPainter::SmoothPixmapTransform | QPainter::TextAntialiasing);
    QPainterPath frame; frame.addRoundedRect(QRectF(tile.rect()).adjusted(1, 1, -1, -1), 16, 16);
    p.fillPath(frame, QColor(0x1b, 0x1f, 0x26));
    p.setClipPath(frame);
    if (!content.isNull()) {
        const auto size = content.size().scaled(tile.size(), Qt::KeepAspectRatio);
        p.drawImage(QRect(QPoint((tile.width() - size.width()) / 2, (tile.height() - size.height()) / 2), size), content);
    }
    if (play) {
        const QPointF centre(tile.width() / 2.0, tile.height() / 2.0 - 14);
        p.setPen(Qt::NoPen); p.setBrush(QColor(0, 0, 0, 150)); p.drawEllipse(centre, 44, 44);
        QPainterPath triangle; triangle.moveTo(centre + QPointF(-14, -22)); triangle.lineTo(centre + QPointF(24, 0)); triangle.lineTo(centre + QPointF(-14, 22)); triangle.closeSubpath();
        p.fillPath(triangle, QColor(0xf2, 0xf4, 0xf6));
    }
    auto font = p.font(); font.setPixelSize(22);
    const QRect strip(0, tile.height() - 48, tile.width(), 48);
    p.fillRect(strip, QColor(0, 0, 0, content.isNull() ? 0 : 150));
    p.setFont(font); p.setPen(QColor(0xe6, 0xe8, 0xeb));
    p.drawText(strip.adjusted(18, 0, -18, 0), Qt::AlignLeft | Qt::AlignVCenter, QFontMetrics(font).elidedText(name, Qt::ElideMiddle, strip.width() - 36));
    if (content.isNull() && !note.isEmpty()) {
        p.setPen(QColor(0x9a, 0xa3, 0xad));
        p.drawText(QRect(18, play ? tile.height() / 2 + 40 : 0, tile.width() - 36, play ? 40 : tile.height() - 48), Qt::AlignCenter, note);
    }
    p.setClipping(false);
    p.setPen(QPen(QColor(0x39, 0x40, 0x4a), 2)); p.setBrush(Qt::NoBrush); p.drawPath(frame);
    return tile;
}
}

namespace {
// A slow, smooth cycle independent of the platform's busy-bar speed.
class CompactionProgress final : public QProgressBar {
public:
    explicit CompactionProgress(QWidget *parent) : QProgressBar(parent) {
        m_timer.setInterval(40);
        connect(&m_timer, &QTimer::timeout, this, qOverload<>(&QWidget::update));
    }
protected:
    void showEvent(QShowEvent *event) override {
        QProgressBar::showEvent(event); m_elapsed.start(); m_timer.start();
    }
    void hideEvent(QHideEvent *event) override {
        m_timer.stop(); QProgressBar::hideEvent(event);
    }
    void paintEvent(QPaintEvent *) override {
        QPainter painter(this); painter.setRenderHint(QPainter::Antialiasing);
        painter.setPen(Qt::NoPen); painter.setBrush(palette().color(QPalette::Base));
        painter.drawRoundedRect(rect(), 2.5, 2.5);
        const qreal phase = (m_elapsed.isValid() ? m_elapsed.elapsed() % 3200 : 0) / 3200.;
        const qreal position = (1 - std::cos(phase * 2 * 3.141592653589793)) / 2;
        const qreal chunk = width() * .4;
        painter.setBrush(palette().color(QPalette::Highlight));
        painter.drawRoundedRect(QRectF(position * (width() - chunk), 0, chunk, height()), 2.5, 2.5);
    }
private:
    QElapsedTimer m_elapsed;
    QTimer m_timer;
};

// Card buttons drawn in the journal colours: "hgs-ui:<copy|copied>/<light|dark>/<scale percent>".
// A query names the card, so that one pressed button can change its image alone.
QImage cardButton(const QUrl &url, qreal devicePixelRatio)
{
    if (url.scheme() != QLatin1String("hgs-ui")) return {};
    const QStringList parts = url.path().split('/');
    if (parts.size() != 3 || (parts[0] != "copy" && parts[0] != "copied") || (parts[1] != "light" && parts[1] != "dark")) return {};
    bool ok = false; const double scale = parts[2].toInt(&ok) / 100.0;
    if (!ok || scale < 0.5 || scale > 4.0) return {};
    const bool dark = parts[1] == "dark";
    const qreal ratio = qMax<qreal>(1.0, devicePixelRatio);
    QImage image(qCeil(14 * scale * ratio), qCeil(14 * scale * ratio), QImage::Format_ARGB32_Premultiplied);
    image.setDevicePixelRatio(ratio); image.fill(Qt::transparent);
    QPainter painter(&image); painter.setRenderHint(QPainter::Antialiasing); painter.scale(scale, scale);
    if (parts[0] == "copied") {
        painter.setPen(QPen(QColor(dark ? "#8bdfc0" : "#167357"), 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        painter.drawPolyline(QPolygonF{QPointF(2.5, 7.5), QPointF(5.5, 10.5), QPointF(11.5, 4)});
        return image;
    }
    // Two sheets; the one behind stops short of the front sheet's outline.
    const QRectF front(4.5, 4.5, 8, 8);
    painter.setPen(QPen(QColor(dark ? "#9eabba" : "#657487"), 1.2)); painter.setBrush(Qt::NoBrush);
    QPainterPath visible, covered; visible.addRect(0, 0, 14, 14); covered.addRect(front.adjusted(-1.5, -1.5, 1.5, 1.5));
    painter.setClipPath(visible.subtracted(covered));
    painter.drawRoundedRect(QRectF(1.5, 1.5, 8, 8), 2, 2);
    painter.setClipping(false);
    painter.drawRoundedRect(front, 2, 2);
    return image;
}

// QTextDocument can otherwise read local files as well as network resources.
// Hook text is display data, never a source of images, CSS or local file reads.
class JournalDocument final : public QTextDocument {
public:
    using QTextDocument::QTextDocument;
    qreal pixelRatio = 1.0, defaultDpi = 96.0;
    QImage metrics;
    JournalDocument *layoutSource = nullptr;
protected:
    // Only drawn Markdown decorations and card buttons load; remote and file resources never do.
    QVariant loadResource(int type, const QUrl &url) override
    {
        if (layoutSource) return layoutSource->resource(type, url);
        if (url.scheme() == QLatin1String("hgs-ui")) return cardButton(url, pixelRatio);
        return MarkdownObjects::resource(url, pixelRatio);
    }
};

class JournalImages final : public QObject, public QTextObjectInterface {
    Q_OBJECT
    Q_INTERFACES(QTextObjectInterface)
public:
    JournalImages(JournalDocument *source, QObject *parent) : QObject(parent), m_source(source) {}
    QSizeF intrinsicSize(QTextDocument *, int, const QTextFormat &format) override {
        const auto image = format.toImageFormat();
        const auto resource = m_source->resource(QTextDocument::ImageResource, QUrl(image.name())).value<QImage>();
        const QSizeF natural = resource.deviceIndependentSize();
        QSize size(qRound(image.hasProperty(QTextFormat::ImageWidth) ? image.width() : natural.width()),
                   qRound(image.hasProperty(QTextFormat::ImageHeight) ? image.height() : natural.height()));
        if (!resource.isNull()) size *= m_source->metrics.logicalDpiY() / m_source->defaultDpi;
        return size;
    }
    void drawObject(QPainter *painter, const QRectF &rect, QTextDocument *, int, const QTextFormat &format) override {
        painter->drawImage(rect, m_source->resource(QTextDocument::ImageResource, QUrl(format.toImageFormat().name())).value<QImage>());
    }
private:
    JournalDocument *m_source;
};

// QTextEdit unconditionally sets the document page size during installation,
// which invalidates even an already finished Qt layout. Keep that layout in an
// owned immutable document and expose the same formatted text through this
// adapter. Widget geometry changes request a new generation rather than laying
// the complete transcript out again on the GUI thread.
class JournalLayout final : public QAbstractTextDocumentLayout {
public:
    JournalLayout(JournalDocument *document, JournalDocument *source)
        : QAbstractTextDocumentLayout(document), m_source(source) {
        source->setParent(this);
        setPaintDevice(&source->metrics);
        registerHandler(QTextFormat::ImageObject, new JournalImages(source, this));
        const auto collect = [&](const auto &self, QTextFrame *frame) -> void {
            m_frames.insert(frame->firstPosition(), frame);
            for (auto *child : frame->childFrames()) self(self, child);
        };
        collect(collect, source->rootFrame());
    }
    void draw(QPainter *painter, const PaintContext &context) override {
        auto mapped = context;
        for (auto &selection : mapped.selections) {
            const auto original = selection.cursor;
            selection.cursor = QTextCursor(m_source);
            selection.cursor.setPosition(original.anchor());
            selection.cursor.setPosition(original.position(), QTextCursor::KeepAnchor);
        }
        m_source->documentLayout()->draw(painter, mapped);
    }
    int hitTest(const QPointF &point, Qt::HitTestAccuracy accuracy) const override { return m_source->documentLayout()->hitTest(point, accuracy); }
    int pageCount() const override { return m_source->documentLayout()->pageCount(); }
    QSizeF documentSize() const override { return m_source->documentLayout()->documentSize(); }
    QRectF frameBoundingRect(QTextFrame *frame) const override {
        auto *source = m_frames.value(frame->firstPosition());
        return source ? m_source->documentLayout()->frameBoundingRect(source) : QRectF();
    }
    QRectF blockBoundingRect(const QTextBlock &block) const override {
        return m_source->documentLayout()->blockBoundingRect(m_source->findBlock(block.position()));
    }
protected:
    void documentChanged(int, int, int) override {}
    void resizeInlineObject(QTextInlineObject item, int position, const QTextFormat &format) override {
        QAbstractTextDocumentLayout::resizeInlineObject(item, position, format);
        const auto character = format.toCharFormat();
        const qreal height = item.ascent() + item.descent();
        if (character.verticalAlignment() == QTextCharFormat::AlignMiddle) {
            const qreal halfX = QFontMetrics(character.font()).xHeight() / 2.;
            item.setAscent((height + halfX) / 2.);
            item.setDescent((height - halfX) / 2.);
        } else if (character.verticalAlignment() == QTextCharFormat::AlignBaseline) {
            const qreal descent = QFontMetricsF(character.font()).descent();
            item.setAscent(height - descent); item.setDescent(descent);
        }
    }
private:
    JournalDocument *m_source;
    QHash<int, QTextFrame *> m_frames;
};

// WhiteWh fork: with the skin's bubbles the dark canvas is the session panel's
// surface, so the feed and the composer below it read as one area.
const char *canvasDark(bool bubbles) { return bubbles ? "#1b1f26" : "#1c2229"; }

JournalDocument *paintDocument(QTextDocument *document)
{
    auto *journal = static_cast<JournalDocument *>(document);
    return journal->layoutSource ? journal->layoutSource : journal;
}

// WhiteWh fork: rounded cards. Qt's rich text draws table backgrounds and
// borders square. With the skin's bubbles, a table with its own background
// gives them up here and the browser paints them rounded beneath the text: 10
// px with a border (cards, bubbles), 5 px without (pills). A first column of 4
// px or less with a background is the card's coloured edge.
enum : int { CardFill = QTextFormat::UserProperty + 401, CardStroke, CardEdge, CardRadius };

// The cards of a document, found once when it is built; frames of a built
// journal do not change.
class CardFrames final : public QObject {
    Q_OBJECT
public:
    explicit CardFrames(QTextDocument *document) : QObject(document) {}
    static CardFrames *of(QTextDocument *document) { return document->findChild<CardFrames *>(QString(), Qt::FindDirectChildrenOnly); }
    QList<QTextFrame *> frames;
};

void roundCards(QTextDocument *document)
{
    // Qt builds the frame tree lazily during an edit: find the tables by their blocks.
    QList<QTextTable *> tables;
    for (auto block = document->begin(); block.isValid(); block = block.next())
        for (auto *table = QTextCursor(block).currentTable(); table; table = qobject_cast<QTextTable *>(table->parentFrame()))
            if (!tables.contains(table)) tables.append(table);
    auto *cards = CardFrames::of(document);
    if (!cards) cards = new CardFrames(document);
    QTextCursor edit(document); edit.beginEditBlock();
    for (auto *table : tables) {
        auto format = table->format();
        if (format.background().style() == Qt::NoBrush) continue;
        format.setProperty(CardFill, format.background().color());
        format.clearBackground();
        const bool bordered = format.border() > 0;
        // The border keeps its width (the layout stays) and loses its colour.
        if (bordered) { format.setProperty(CardStroke, format.borderBrush().color()); format.setBorderBrush(Qt::transparent); }
        format.setProperty(CardRadius, bordered ? 10.0 : 5.0);
        const auto widths = format.columnWidthConstraints();
        if (table->columns() >= 2 && !widths.isEmpty() && widths.first().type() == QTextLength::FixedLength && widths.first().rawValue() <= 4) {
            auto cell = table->cellAt(0, 0);
            auto cellFormat = cell.format();
            if (cellFormat.background().style() != Qt::NoBrush) {
                format.setProperty(CardEdge, cellFormat.background().color());
                cellFormat.clearBackground(); cell.setFormat(cellFormat);
            }
        }
        table->setFormat(format);
        cards->frames.append(table);
    }
    edit.endEditBlock();
}

void paintCards(QPainter *painter, QTextDocument *document, const QRectF &exposed)
{
    const auto *cards = CardFrames::of(document);
    if (!cards || cards->frames.isEmpty()) return;
    auto *layout = document->documentLayout();
    painter->save(); painter->setRenderHint(QPainter::Antialiasing);
    for (auto *frame : cards->frames) {
        const auto format = frame->frameFormat();
        QRectF rect = layout->frameBoundingRect(frame);
        // Qt places a table's rectangle at its first cell's content: step back
        // over that cell's padding and the border.
        if (auto *table = qobject_cast<QTextTable *>(frame)) {
            const auto cell = table->cellAt(0, 0).format().toTableCellFormat();
            const qreal padding = table->format().cellPadding(), border = table->format().border();
            const qreal left = cell.hasProperty(QTextFormat::TableCellLeftPadding) ? cell.leftPadding() : padding;
            const qreal top = cell.hasProperty(QTextFormat::TableCellTopPadding) ? cell.topPadding() : padding;
            rect.translate(-(left + border), -(top + border));
        }
        if (!rect.intersects(exposed)) continue;
        const qreal radius = format.doubleProperty(CardRadius);
        QPainterPath path; path.addRoundedRect(rect.adjusted(.5, .5, -.5, -.5), radius, radius);
        painter->fillPath(path, format.colorProperty(CardFill));
        if (format.hasProperty(CardEdge)) {
            painter->save(); painter->setClipPath(path);
            painter->fillRect(QRectF(rect.left(), rect.top(), 3, rect.height()), format.colorProperty(CardEdge));
            painter->restore();
        }
        if (format.hasProperty(CardStroke)) { painter->setPen(QPen(format.colorProperty(CardStroke), 1)); painter->setBrush(Qt::NoBrush); painter->drawPath(path); }
    }
    painter->restore();
}

// WhiteWh fork: the glyph of a tool row (edit, read, run, search), drawn on the
// render worker, so into an image rather than a pixmap.
QImage toolGlyph(const QString &kind, const QColor &color, qreal ratio)
{
    QImage image(qCeil(14 * ratio), qCeil(14 * ratio), QImage::Format_ARGB32_Premultiplied);
    image.setDevicePixelRatio(ratio); image.fill(Qt::transparent);
    QPainter p(&image); p.setRenderHint(QPainter::Antialiasing); p.scale(14 / 24., 14 / 24.);
    p.setPen(QPen(color, 1.8, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin)); p.setBrush(Qt::NoBrush);
    if (kind == "edit") {
        p.drawPolygon(QPolygonF{QPointF(4, 20), QPointF(5, 15), QPointF(15, 5), QPointF(19, 9), QPointF(9, 19)});
        p.drawLine(QPointF(13, 7), QPointF(17, 11));
    } else if (kind == "read") {
        QPainterPath eye; eye.moveTo(2, 12); eye.quadTo(12, 3, 22, 12); eye.quadTo(12, 21, 2, 12); p.drawPath(eye);
        p.drawEllipse(QPointF(12, 12), 3, 3);
    } else if (kind == "search") {
        p.drawEllipse(QPointF(10, 10), 6, 6); p.drawLine(QPointF(14.5, 14.5), QPointF(20, 20));
    } else {
        p.drawEllipse(QPointF(12, 12), 9, 9);
        p.drawPolygon(QPolygonF{QPointF(10, 8), QPointF(16, 12), QPointF(10, 16)});
    }
    return image;
}

// Copies what the reader sees instead of U+FFFC for markers, and draws the
// chips of inline code, which stays text so that it wraps and selects freely.
class ActivityBrowser final : public QTextBrowser {
public:
    using QTextBrowser::QTextBrowser;
    std::function<void(QMenu *)> extendMenu;
    // The margins beside the reading column. The scroll bar keeps the pane edge.
    void setColumnMargins(const QMargins &margins) { if (viewportMargins() != margins) setViewportMargins(margins); }
protected:
    QMimeData *createMimeDataFromSelection() const override
    {
        auto *data = new QMimeData;
        data->setText(MarkdownObjects::plainText(textCursor()));
        return data;
    }
    void paintEvent(QPaintEvent *event) override
    {
        // WhiteWh fork: rounded cards beneath the text.
        {
            QPainter cards(viewport());
            const QPoint offset(horizontalScrollBar()->value(), verticalScrollBar()->value());
            cards.translate(-offset);
            paintCards(&cards, paintDocument(document()), QRectF(event->rect().translated(offset)));
        }
        QTextBrowser::paintEvent(event);
        // The selections as QTextEdit paints them: found text, then the reader's.
        QList<QAbstractTextDocumentLayout::Selection> selections;
        for (const auto &extra : extraSelections()) selections.append({extra.cursor, extra.format});
        if (textCursor().hasSelection()) {
            const auto group = hasFocus() ? QPalette::Active : QPalette::Inactive;
            QTextCharFormat format;
            format.setBackground(palette().brush(group, QPalette::Highlight));
            format.setForeground(palette().brush(group, QPalette::HighlightedText));
            QStyleOption option; option.initFrom(this);
            if (style()->styleHint(QStyle::SH_RichText_FullWidthSelection, &option, this)) format.setProperty(QTextFormat::FullWidthSelection, true);
            selections.append({textCursor(), format});
        }
        QPainter painter(viewport());
        const QPoint scroll(horizontalScrollBar()->value(), verticalScrollBar()->value());
        painter.translate(-scroll);
        MarkdownObjects::paintChips(&painter, paintDocument(document()), QRectF(event->rect().translated(scroll)), viewport()->palette(), selections);
    }
    void contextMenuEvent(QContextMenuEvent *event) override
    {
        auto *menu = createStandardContextMenu(event->pos() + QPoint(horizontalScrollBar()->value(), verticalScrollBar()->value()));
        if (extendMenu) extendMenu(menu);
        menu->setAttribute(Qt::WA_DeleteOnClose);
        menu->popup(event->globalPos());
    }
};

// A reading column edge in the transcript margin. Both edges move together,
// so the column stays centered; a double-click restores the default width.
class ColumnEdge final : public QWidget {
public:
    static constexpr int Width = 10;
    std::function<void()> pressed;
    std::function<void(int)> dragged;   // outward pointer distance since the press
    std::function<void()> reset;
    std::function<void(const QPoint &)> menuRequested;
    QColor color;
    ColumnEdge(bool right, QWidget *parent) : QWidget(parent), m_right(right)
    {
        setObjectName("activityColumnEdge"); setCursor(Qt::SizeHorCursor);
        setToolTip(QObject::tr("Drag to resize Activity. Double-click to restore the default width."));
        hide();
    }
protected:
    void enterEvent(QEnterEvent *) override { m_hover = true; update(); }
    void leaveEvent(QEvent *) override { m_hover = false; update(); }
    // An edge hidden under the pointer never receives its leave event.
    void hideEvent(QHideEvent *) override { m_hover = m_dragging = false; }
    void mousePressEvent(QMouseEvent *event) override
    {
        if (event->button() != Qt::LeftButton) { event->ignore(); return; }
        m_origin = event->globalPosition().x(); m_dragging = true; update();
        if (pressed) pressed();
    }
    void mouseMoveEvent(QMouseEvent *event) override
    {
        if (!m_dragging || !dragged) return;
        const int distance = qRound(event->globalPosition().x() - m_origin);
        dragged(m_right ? distance : -distance);
    }
    void mouseReleaseEvent(QMouseEvent *event) override
    {
        if (event->button() == Qt::LeftButton) { m_dragging = false; update(); }
    }
    void mouseDoubleClickEvent(QMouseEvent *event) override
    {
        if (event->button() == Qt::LeftButton && reset) reset();
    }
    void contextMenuEvent(QContextMenuEvent *event) override { if (menuRequested) menuRequested(event->globalPos()); }
    void paintEvent(QPaintEvent *) override
    {
        if (!m_hover && !m_dragging) return;
        QPainter painter(this); QColor line = color; line.setAlpha(m_dragging ? 255 : 150);
        painter.fillRect(QRect(width() / 2 - 1, 0, 2, height()), line);
    }
private:
    bool m_right, m_hover = false, m_dragging = false;
    qreal m_origin = 0;
};

bool webLink(const QUrl &url)
{
    return url.isValid() && !url.host().isEmpty() && url.userInfo().isEmpty()
        && (url.scheme() == "https" || url.scheme() == "http");
}

QString escaped(const QString &text) { return text.toHtmlEscaped(); }

// WhiteWh fork: agent teams. Zerus prepends a literal header to every team
// message ("[Zerus team · from: Lead · task: #3 · attempt: 2]"); such a
// message reads as a plaque: kind, sender and receiver, task and attempt.
struct TeamMessage {
    bool valid = false;
    QString header, from, to, kind, title, body, full;
    int task = 0, attempt = 0;
};

TeamMessage teamMessage(QString text)
{
    TeamMessage m;
    text = text.trimmed();
    // A message from a session arrives in Zerus's envelope.
    static const QRegularExpression envelope(QStringLiteral("^<zerus-message[^>]*>\\s*([\\s\\S]*?)\\s*</zerus-message>$"));
    if (const auto found = envelope.match(text); found.hasMatch()) text = found.captured(1).trimmed();
    static const QRegularExpression header(QStringLiteral("^\\[Zerus team \\x{00b7} from: ([^\\x{00b7}\\]]+?)((?: \\x{00b7} [^\\x{00b7}\\]]+)*)\\]"));
    const auto match = header.match(text);
    if (!match.hasMatch()) return m;
    m.valid = true; m.header = match.captured(0); m.from = match.captured(1).trimmed(); m.full = text;
    const auto parts = match.captured(2).split(QChar(0x00b7), Qt::SkipEmptyParts);
    bool planning = false, decided = false;
    for (auto part : parts) {
        part = part.trimmed();
        if (part.startsWith("task: #")) m.task = part.mid(7).toInt();
        else if (part.startsWith("attempt: ")) m.attempt = part.mid(9).toInt();
        else if (part == "planning") planning = true;
        else if (part == "all tasks decided") decided = true;
    }
    auto rest = text.mid(match.capturedLength()).trimmed();
    static const QRegularExpression role(QStringLiteral("Your role: ([^.\\n]+)\\."));
    static const QRegularExpression taskLine(QStringLiteral("(?:^|\\n)Task #\\d+: ([^\\n]+)"));
    static const QRegularExpression reportLine(QStringLiteral("^Report for task #\\d+ ([^\\n]*):\\s*"));
    if (const auto r = reportLine.match(rest); r.hasMatch()) {
        m.kind = QObject::tr("Report to lead"); m.to = QObject::tr("Lead"); m.title = r.captured(1);
        rest = rest.mid(r.capturedLength());
        const int decide = rest.lastIndexOf(QStringLiteral("\n\nDecide with "));
        m.body = (decide >= 0 ? rest.left(decide) : rest).trimmed();
    } else if (planning) {
        m.kind = QObject::tr("Planning brief"); m.to = QObject::tr("Lead");
        const int plan = rest.indexOf(QStringLiteral("\n\nPlan the work"));
        m.body = (plan >= 0 ? rest.left(plan) : rest).trimmed();
    } else if (decided) {
        m.kind = QObject::tr("All tasks decided");
        m.to = QObject::tr("Lead");
        // A team name may contain a period ("v1.2 release"); the sentence after
        // the name starts with ". All ".
        static const QRegularExpression teamLine(QStringLiteral("^Team: (.+?)\\.\\s+"));
        if (const auto t = teamLine.match(rest); t.hasMatch()) rest = rest.mid(t.capturedLength());
        m.body = rest.trimmed();
    } else if (m.from == QStringLiteral("You")) {
        m.kind = QObject::tr("Message from you"); m.body = rest;
    } else {
        m.kind = QObject::tr("Task from team");
        if (const auto r = role.match(rest); r.hasMatch()) m.to = r.captured(1).trimmed();
        if (const auto t = taskLine.match(rest); t.hasMatch()) {
            m.title = t.captured(1).trimmed();
            rest = rest.mid(t.capturedEnd()).trimmed();
        }
        const int report = rest.indexOf(QStringLiteral("When you finish, report once"));
        m.body = (report >= 0 ? rest.left(report) : rest).trimmed();
    }
    return m;
}

QString teamRoleColor(const QString &role, bool dark)
{
    static const QList<std::tuple<const char *, const char *, const char *>> palette{
        {"lead", "#b79cff", "#6f4fd8"}, {"architect", "#8e9bff", "#3d4bc4"}, {"backend", "#6fa8ff", "#2f66c9"},
        {"frontend", "#f08bb4", "#b83a72"}, {"tester", "#56c8e0", "#137f95"}, {"reviewer", "#d9b38c", "#8a5a2b"},
        {"security", "#e48de0", "#a2349c"}, {"integrator", "#a3b1c6", "#4d5d75"}, {"researcher", "#b9d36b", "#5f7a12"}};
    const auto key = role.toLower();
    if (key == "you") return dark ? "#aab1ba" : "#4d5c6b";
    for (const auto &[name, d, l] : palette) if (key.contains(QLatin1String(name))) return dark ? d : l;
    const auto &[name, d, l] = palette[1 + qHash(key) % (palette.size() - 1)];
    Q_UNUSED(name);
    return dark ? d : l;
}

QString userMessage(const QString &text)
{
    // The composer accepts plain text. Markdown would consume quote markers,
    // list numbers and line breaks that are part of the user's actual request.
    return "<p style='white-space:pre-wrap;'>" + escaped(text) + "</p>";
}

QString questionReplyBody(const QJsonArray &replies, const QString &muted, const QString &accent,
                          const QString &surface, const QString &border)
{
    QString html;
    for (const auto &value : replies) {
        const auto reply = value.toObject();
        html += QString("<p style='font-size:10px;color:%1;margin-bottom:3px;'>%2</p>"
            "<p style='color:%1;white-space:pre-wrap;margin-top:0;margin-bottom:10px;'>%3</p>"
            "<table width='100%' cellspacing='0' cellpadding='10' style='border:1px solid %4;'><tr><td bgcolor='%5'>"
            "<p style='font-size:10px;color:%6;margin-top:0;margin-bottom:4px;'><b>%7</b></p>"
            "<p style='white-space:pre-wrap;margin:0;'>%8</p></td></tr></table>")
            .arg(muted, escaped(QObject::tr("Question")), escaped(reply.value("question").toString()),
                 border, surface, accent, escaped(QObject::tr("Your answer")), escaped(reply.value("answer").toString()));
    }
    return html;
}

QString eventKey(const QJsonObject &event)
{
    if(event.value("source")=="hgs_voice")return "voice-"+event.value("voice").toObject().value("id").toString(event.value("message_id").toString());
    if (!event.value("activity_key").toString().isEmpty()) return event.value("activity_key").toString();
    if(event.value("type")=="LocalMessage")return "local-"+event.value("id").toString();
    if(event.value("source")=="hgs_delivery")return "receipt-"+event.value("message_id").toString();
    if (event.value("source") == "kimi_wire" && !event.value("message_id").toString().isEmpty())
        return "kimi-" + event.value("message_id").toString();
    const QString nativeReply = event.value("reply_id").toString();
    if (nativeReply.startsWith("dsh-turn-") && nativeReply.mid(9).toLongLong() >= 0 && event.value("source") == "dsh_native") return nativeReply;
    if (event.value("seq").toInteger() > 0) return QString::number(event.value("seq").toInteger());
    return QString::fromLatin1(QCryptographicHash::hash(QJsonDocument(event).toJson(QJsonDocument::Compact),
                                                       QCryptographicHash::Sha256).toHex().left(20));
}

// A clear first confirmed in Terminal is journaled in the old conversation,
// and older versions journaled it again under the new one. The same moment
// is one boundary, whatever key either copy carries.
bool sameSessionClear(const QJsonObject &event, const QJsonObject &clear)
{
    return event.value("type") == "SessionCleared" && (event.value("activity_key") == clear.value("activity_key")
        || (event.value("run_id") == clear.value("run_id") && event.value("at").toDouble() > 0 && event.value("at") == clear.value("at")));
}

QString eventTitle(const QString &type)
{
    static const QMap<QString, QString> titles{
        {"SessionStart", "Session started"}, {"SessionEnd", "Session closed"},
        {"TurnStarted", "Turn started"}, {"UserPromptQueued", "Message queued"},
        {"PreToolUse", "Tool started"}, {"PostToolUse", "Tool finished"},
        {"PostToolUseFailure", "Tool failed"}, {"PermissionRequest", "Approval requested"},
        {"PermissionResult", "Approval resolved"}, {"Stop", "Response finished"},
        {"StopFailure", "Response failed"}, {"Interrupt", "Turn interrupted"},
        {"PreCompact", "Compacting context"}, {"PostCompact", "Context compacted"},
        {"SubagentStart", "Subagent started"}, {"SubagentStop", "Subagent finished"},
        {"TaskStarted", "Background task started"}, {"Notification", "Agent notification"}};
    return titles.value(type, type.isEmpty() ? QObject::tr("Activity") : type);
}

QString timeText(const QJsonObject &event)
{
    const double at = event.value("at").toDouble();
    if (at <= 0) return {};
    return QDateTime::fromSecsSinceEpoch(qint64(at)).toLocalTime().toString("d MMM HH:mm:ss");
}

QString messageRole(const QJsonObject &event)
{
    if (event.value("source") == "zerus_task" && event.value("type") == "TaskEntry") {
        const auto role = event.value("task_role").toString();
        return role == "person" ? QStringLiteral("user") : role == "agent" ? QStringLiteral("assistant") : QString();
    }
    if (event.value("source") == "hgs_voice" && event.value("voice").isObject()) return "user";
    if (!event.value("agent_id").toString().isEmpty() || (event.value("detail").toString().isEmpty() && event.value("attachments").toArray().isEmpty())) return {};
    // Claude starts a turn for finished background work; nobody wrote it.
    const auto origin = event.value("origin").toString();
    if (origin == "task_notification" || origin == "subagent_report" || origin == "peer_message" || origin == "cache_keepalive") return "notice";
    const auto type = event.value("type").toString();
    // Kimi reports an empty UserPromptSubmit followed by the actual prompt in
    // TurnStarted. Keep that prompt in the timeline, ahead of its tool calls.
    if (type == "UserPromptSubmit" || type == "UserPromptQueued" || type == "TurnStarted" || type == "QuestionAnswered" || type=="LocalMessage") return "user";
    if (type == "Stop" || type == "AgentMessage") return "assistant";
    return {};
}

bool attention(const QJsonObject &event)
{
    const auto type = event.value("type").toString();
    return type == "PermissionRequest" || type == "PostToolUseFailure" || type == "StopFailure" || type == "Interrupt";
}

// A snapshot contains a longer version of the latest journal excerpt. Expand
// only a matching prefix; never attribute an older response to a newer turn.
bool isExcerptOf(QString excerpt, const QString &full)
{
    if (excerpt.endsWith(QChar(0x2026))) excerpt.chop(1);
    return !excerpt.isEmpty() && full.startsWith(excerpt);
}

// WhiteWh fork: the pictures a tool group worked with, from its results: a
// DeepSeek read (<path>…</path> with <type>image</type>), a file written, or
// any absolute path to a picture in a finished tool's output. At most four.
QStringList toolMedia(const QList<QJsonObject> &group)
{
    static const QRegularExpression read(QStringLiteral("<path>([^<\\r\\n]+)</path>\\s*<type>image</type>"));
    static const QRegularExpression path(QStringLiteral("(?:[A-Za-z]:[\\\\/]|/)[^\\s\"'<>|*?]*\\.(?:png|jpe?g|gif|webp)"),
                                         QRegularExpression::CaseInsensitiveOption);
    QStringList media;
    const auto add = [&](const QString &href) {
        const auto file = SessionFileReference::parse(href.trimmed());
        if (media.size() < 4 && file.valid() && MediaFiles::kind(file.path) != MediaFiles::Kind::None && MediaFiles::kind(file.path) != MediaFiles::Kind::Video && !media.contains(href.trimmed()))
            media.append(href.trimmed());
    };
    for (const auto &event : group) {
        if (event.value("type") != "PostToolUse") continue;
        const auto detail = event.value("detail").toString();
        for (auto it = read.globalMatch(detail); it.hasNext();) add(it.next().captured(1));
        for (auto it = path.globalMatch(detail); it.hasNext();) add(it.next().captured(0));
    }
    return media;
}

// WhiteWh fork: `media` collects the file references that name a picture,
// animation or video, for the previews under the message.
QString markdown(const QString &text, const MarkdownTheme &theme, QHash<QString, QString> &fileLinks, QStringList *media = nullptr)
{
    return MarkdownHtml::render(text, theme, [&fileLinks, media](const QString &href) -> MarkdownLink {
        const auto file = SessionFileReference::parse(href);
        if (file.valid() && media && MediaFiles::kind(file.path) != MediaFiles::Kind::None && !media->contains(href)) media->append(href);
        if (file.valid()) {
            const QString key = QString::fromLatin1(QCryptographicHash::hash(href.toUtf8(), QCryptographicHash::Sha256).toHex());
            fileLinks.insert(key, href);
            return {"hgs-file:" + key, file.path + file.location, file.path + file.location};
        }
        if (webLink(QUrl(href))) return {href, {}, {}};
        return {};
    });
}

// Named anchors let a reading position survive journal pruning and expansion
// above it. They also preserve both endpoints of a text selection on refresh.
QMap<QString, int> bookmarks(QTextDocument *document)
{
    QMap<QString, int> result;
    for (auto block = document->begin(); block.isValid(); block = block.next())
        for (auto it = block.begin(); !it.atEnd(); ++it) {
            const auto f = it.fragment();
            for (const auto &name : f.charFormat().anchorNames())
                if (name.startsWith("item-")) result.insert(name, f.position());
        }
    return result;
}

struct Position {
    QString key;
    int offset = 0, absolute = 0;
    Position(int position, const QMap<QString, int> &anchors) : absolute(position) {
        int nearest = -1;
        for (auto it = anchors.cbegin(); it != anchors.cend(); ++it)
            if (it.value() <= position && it.value() > nearest) { key = it.key(); nearest = it.value(); }
        offset = nearest < 0 ? position : position - nearest;
    }
    int restored(const QMap<QString, int> &anchors, int length) const {
        return qBound(0, anchors.contains(key) ? anchors.value(key) + offset : absolute, qMax(0, length - 1));
    }
};
}

// Values captured on the GUI thread; each job owns its document exclusively
// until construction and layout finish. No widget or pixmap enters the worker.
struct ActivityRenderResult {
    QString m_sessionKey, m_conversation, m_fallbackPrompt, m_html;
    QJsonObject m_details, m_searchResult;
    QJsonArray m_events, m_localMessages;
    QHash<QString, bool> m_expanded;
    QHash<QString, QString> m_fileLinks, m_deliveryKeys, m_attachmentBindings, m_copyTexts;
    QHash<QString, QJsonObject> m_attachmentLinks, m_previewFiles, m_voiceLinks;
    QHash<QString,int> m_voicePositions;
    QString m_voiceError;
    QHash<QString, QPair<QString, QString>> m_messageActions;
    QHash<QString, QImage> m_previews;
    // WhiteWh fork: media previews by key, and the reference each one shows.
    QHash<QString, QImage> m_mediaTiles;
    QHash<QString, QString> m_mediaLinks;
    QSet<QString> m_knownEvents, m_toggleKeys, m_processLinks;
    bool m_dark = false, m_tracked = true, processesEnabled = false, content = false, m_bubbles = false;
    double m_scale = 1.;
    quint64 generation = 0, contentRevision = 0;
    qreal width = 0;
    QSizeF size;
    qreal widthWithScrollBar = 0;
    int viewportHeight = 0;
    QMap<QString, int> anchors;
    std::unique_ptr<JournalDocument> document;
    static QString tr(const char *text) { return ActivityView::tr(text); }
    QString copyButton(const QString &state, const QString &key) const {
        return QString("hgs-ui:%1/%2/%3?card=%4").arg(state, m_dark ? "dark" : "light", QString::number(qRound(m_scale * 100)), key);
    }
    void build() {
    const bool searching = !m_searchResult.isEmpty();
    const QJsonObject details = searching ? QJsonObject() : m_details;
    // A refetched journal window can overlap the previous one. Deduplicate
    // native identities before receipts replace their display keys and text.
    QSet<qint64> journalSequences;
    QSet<QString> nativeTurns;
    QJsonArray uniqueEvents;
    for (const auto &value : m_events) {
        auto event = value.toObject();
        const auto seq = event.value("seq").toInteger();
        if (seq > 0 && journalSequences.contains(seq)) continue;
        if (seq > 0) journalSequences.insert(seq);
        // Older journals discarded Kimi's native turn ID. A complete bounded
        // native file can still prove a unique attachment turn: its immutable
        // staged-file expansion must match exactly, with no competing send.
        if (details.value("agent") == "kimi" && details.value("kimi_turns_complete").toBool()
            && event.value("type") == "TurnStarted" && !event.contains("kimi_turn_id")
            && event.value("agent_id").toString().isEmpty()
            && !details.value("run_id").toString().isEmpty() && !details.value("conversation_id").toString().isEmpty()
            && event.value("run_id") == details.value("run_id")) {
            const auto body = event.value("detail").toString().trimmed();
            QSet<QString> receipts;
            for (const auto &receipt : details.value("attachment_messages").toArray()) {
                const auto row = receipt.toObject();
                const auto id = row.value("message_id").toString();
                if (row.value("source") == "hgs_delivery" && !row.value("attachments").toArray().isEmpty()
                    && !id.isEmpty() && !body.isEmpty() && row.value("submitted_text").toString().trimmed() == body) receipts.insert(id);
            }
            // A confirmed send can be visible before the next inspection has
            // its durable file receipt. Both copies share the same delivery ID.
            for (const auto &message : m_localMessages) {
                const auto row = message.toObject();
                const auto id = row.value("message_id").toString();
                if (row.value("status") == "sent" && row.value("source") != "hgs_voice"
                    && row.value("run_id") == details.value("run_id")
                    && row.value("conversation_id") == details.value("conversation_id")
                    && !row.value("attachments").toArray().isEmpty() && !id.isEmpty()
                    && !body.isEmpty() && row.value("submitted_text").toString().trimmed() == body) receipts.insert(id);
            }
            if (receipts.size() == 1) {
                const auto hash = QString::fromLatin1(QCryptographicHash::hash(body.toUtf8(), QCryptographicHash::Sha256).toHex());
                QList<QJsonObject> matching;
                for (const auto &turn : details.value("kimi_turns").toArray())
                    if (turn.toObject().value("text_sha256").toArray().contains(hash)) matching.append(turn.toObject());
                if (matching.size() == 1) {
                    const auto turn = matching.first();
                    const double delay = event.value("at").toDouble() - turn.value("at").toDouble();
                    if (turn.value("origin") == "user" && turn.value("turn_id").toInteger(-1) >= 0
                        && turn.value("at").toDouble() > 0 && delay >= 0 && delay <= 2.)
                    {
                        event["kimi_turn_id"] = turn.value("turn_id");
                        event["kimi_conversation_id"] = details.value("conversation_id");
                    }
                }
            }
        }
        const auto turn = event.value("kimi_turn_id").toInteger(-1);
        if (event.value("type") == "TurnStarted" && turn >= 0 && !event.value("run_id").toString().isEmpty()
            && !event.value("kimi_conversation_id").toString().isEmpty()) {
            // A repeated caption with a different native turn is a new send.
            // Conflicting bodies for one ID also stay visible for inspection.
            const auto key = QString::fromLatin1(QCryptographicHash::hash(QJsonDocument(QJsonArray{
                event.value("run_id"), event.value("kimi_conversation_id"), event.value("agent_id"), turn,
                event.value("detail")}).toJson(QJsonDocument::Compact),
                QCryptographicHash::Sha256).toHex());
            if (nativeTurns.contains(key)) continue;
            nativeTurns.insert(key);
        }
        uniqueEvents.append(event);
    }
    m_events = std::move(uniqueEvents);
    const auto clear = details.value("session_clear").toObject();
    if (clear.value("type") == "SessionCleared") m_fallbackPrompt.clear();
    if (clear.value("type") == "SessionCleared" && !std::any_of(m_events.cbegin(), m_events.cend(), [&clear](const QJsonValue &value) {
            return sameSessionClear(value.toObject(), clear);
        })) m_events.append(clear);
    for(const auto &value:details.value("voice_messages").toArray()) {
        const auto voice=value.toObject();const auto id=voice.value("message_id").toString();
        if(id.isEmpty())continue;
        for(qsizetype i=m_events.size();i-->0;)if(m_events[i].toObject().value("source")=="hgs_voice" && m_events[i].toObject().value("message_id")==id)m_events.removeAt(i);
        m_events.append(voice);
    }
    for(const auto &value:details.value("attachment_messages").toArray()) {
        const auto receipt=value.toObject();if(receipt.value("source")!="hgs_delivery" || receipt.value("attachments").toArray().isEmpty())continue;
        const auto submitted=receipt.value("submitted_text").toString().trimmed();int match=-1;double nearest=10.;
        QList<int> exact;
        if (!submitted.isEmpty()) for (int i = 0; i < m_events.size(); ++i) {
            const auto event = m_events[i].toObject();
            if (event.value("source") != "hgs_voice" && event.value("agent_id").toString().isEmpty()
                && event.value("attachments").toArray().isEmpty()
                && QStringList{"UserPromptSubmit", "UserPromptQueued", "TurnStarted"}.contains(event.value("type").toString())
                && event.value("detail").toString().trimmed() == submitted) exact.append(i);
        }
        // Staged attachment paths make the complete submitted text specific to
        // this receipt, even when a queued native prompt is acknowledged later.
        int competing = 0;
        for (const auto &other : details.value("attachment_messages").toArray())
            if (other.toObject().value("submitted_text").toString().trimmed() == submitted) ++competing;
        const auto requestId = receipt.value("message_id").toString();
        if (competing == 1) {
            const auto bound = m_attachmentBindings.value(requestId);
            for (const auto candidate : exact)
                if (!bound.isEmpty() && eventKey(m_events[candidate].toObject()) == bound) match = candidate;
            if (match < 0 && exact.size() == 1) match = exact.first();
            // Adjacent submission/turn-start hooks acknowledge one prompt.
            // Two separate submission IDs are kept independently.
            if (match < 0 && exact.size() == 2 && exact[1] == exact[0] + 1) {
                const auto first = m_events[exact[0]].toObject(), second = m_events[exact[1]].toObject();
                if (QStringList{"UserPromptSubmit", "UserPromptQueued"}.contains(first.value("type").toString())
                    && second.value("type") == "TurnStarted"
                    && first.value("seq").toInteger() > 0 && second.value("seq").toInteger() == first.value("seq").toInteger() + 1)
                    match = exact[0];
            }
        }
        if (match >= 0 && !requestId.isEmpty()) m_attachmentBindings.insert(requestId, eventKey(m_events[match].toObject()));
        for(int i=0;match < 0 && exact.isEmpty() && i<m_events.size();++i) {
            const auto event=m_events[i].toObject();const auto body=event.value("detail").toString().trimmed();
            const double distance=qAbs(event.value("at").toDouble()-receipt.value("at").toDouble());
            if(!event.value("agent_id").toString().isEmpty() || !QStringList{"UserPromptSubmit","UserPromptQueued","TurnStarted"}.contains(event.value("type").toString()))continue;
            if(event.value("source")!="hgs_voice" && event.value("attachments").toArray().isEmpty() && distance<=nearest && !submitted.isEmpty() && !body.isEmpty() && (body.startsWith(submitted.left(1000)) || submitted.startsWith(body.left(1000)))){match=i;nearest=distance;}
        }
        if(match<0){auto message=receipt;message["attachment_submitted_text"]=receipt.value("submitted_text");m_events.append(message);}
        else {auto event=m_events[match].toObject();event["detail"]=receipt.value("detail");event["attachments"]=receipt.value("attachments");event["attachment_request_id"]=receipt.value("message_id");event["attachment_submitted_text"]=receipt.value("submitted_text");m_events[match]=event;}
    }
    for (const auto &value : details.value("provider_messages").toArray()) {
        const auto message = value.toObject();
        if(message.value("type")=="AgentThinking"&&message.value("source")=="claude_transcript") {
            m_events.append(message);continue;
        }
        if (message.value("type") != "AgentMessage" || !QStringList{"kimi_wire", "codex_transcript", "claude_transcript"}.contains(message.value("source").toString())) continue;
        bool matched = false;
        for (int i = 0; i < m_events.size(); ++i) {
            auto event = m_events[i].toObject();
            if (event.value("type") != "Stop" || !event.value("agent_id").toString().isEmpty()
                || qAbs(event.value("at").toDouble() - message.value("at").toDouble()) > 2.0) continue;
            const QString excerpt = event.value("detail").toString();
            // A progress message just before Stop is not the final response.
            // Keep it at its original position instead of filling an empty Stop.
            if (excerpt.isEmpty() && message.value("message_phase") == "commentary") continue;
            if (!excerpt.isEmpty() && !isExcerptOf(excerpt, message.value("detail").toString())) continue;
            event["detail"] = message.value("detail"); event["source"] = message.value("source");
            m_events[i] = event; matched = true; break;
        }
        if (!matched) m_events.append(message);
    }
    for (const auto &value : details.value("provider_errors").toArray()) {
        const auto error = value.toObject();
        if (error.value("type") == "StopFailure" && (error.value("source") == "codex_native_log"
            || error.value("source") == "codex_native_transcript")) m_events.append(error);
    }
    QJsonArray sourceEvents = searching ? QJsonArray{m_searchResult} : m_events;
    const QJsonArray localMessages = searching ? QJsonArray() : m_localMessages;
    for (const auto &value : localMessages) {
        const auto message = value.toObject();
        if (!message["message_id"].toString().isEmpty())
            m_deliveryKeys.insert(message["message_id"].toString(), "local-" + message["id"].toString());
    }
    for (int i = 0; i < sourceEvents.size(); ++i) {
        auto event = sourceEvents[i].toObject();
        // WhiteWh fork: a project's iron rules precede every message Zerus
        // sends; the text is shown, and matched to its delivery, without them.
        if (event.value("detail").isString()) {
            bool rules = false;
            const auto detail = IronRules::strip(event.value("detail").toString(), &rules);
            if (rules) { event["detail"] = detail; event["iron_rules"] = true; sourceEvents[i] = event; }
        }
        const auto id = event.value("attachment_request_id").toString(event.value("message_id").toString());
        if (m_deliveryKeys.contains(id)) { event["activity_key"] = m_deliveryKeys.value(id); sourceEvents[i] = event; }
    }
    for(const auto &value:localMessages) {
        auto message=value.toObject();message["type"]="LocalMessage";message["detail"]=message.value("text");message["at"]=message.value("submitted_at");
        if(message.value("source")=="hgs_voice") {
            bool saved=false;for(const auto &event:sourceEvents)if(event.toObject().value("source")=="hgs_voice" && event.toObject().value("message_id")==message.value("message_id")){saved=true;break;}
            if(!saved)sourceEvents.append(message);
            continue;
        }
        // The native hook can arrive before the durable delivery metadata.
        // Keep the cached file attached to that event, not as a second message.
        int match=-1;double nearest=10.;
        const auto submitted=message.value("submitted_text").toString(message.value("text").toString()).trimmed();
        QList<int> exact;
        if (message.value("status") == "sent" && !message.value("attachments").toArray().isEmpty()) {
            for (int i = 0; i < sourceEvents.size(); ++i) {
                const auto event = sourceEvents[i].toObject();
                if (event.value("source") != "hgs_voice" && event.value("agent_id").toString().isEmpty()
                    && event.value("attachments").toArray().isEmpty()
                    && QStringList{"UserPromptSubmit", "UserPromptQueued", "TurnStarted"}.contains(event.value("type").toString())
                    && !submitted.isEmpty() && event.value("detail").toString().trimmed() == submitted) exact.append(i);
            }
            int competing = 0;
            for (const auto &other : localMessages) {
                const auto candidate = other.toObject();
                if (candidate.value("status") == "sent" && !candidate.value("attachments").toArray().isEmpty()
                    && candidate.value("submitted_text").toString(candidate.value("text").toString()).trimmed() == submitted) ++competing;
            }
            if (competing == 1) {
                const auto bound = m_attachmentBindings.value(message.value("message_id").toString());
                for (const auto candidate : exact)
                    if (!bound.isEmpty() && eventKey(sourceEvents[candidate].toObject()) == bound) match = candidate;
                if (match < 0 && exact.size() == 1) match = exact.first();
                if (match >= 0 && !message.value("message_id").toString().isEmpty())
                    m_attachmentBindings.insert(message.value("message_id").toString(), eventKey(sourceEvents[match].toObject()));
            }
        }
        if(message.value("status")=="sent" && match < 0 && exact.isEmpty())for(int i=0;i<sourceEvents.size();++i) {
            const auto event=sourceEvents[i].toObject();
            if(!event.value("agent_id").toString().isEmpty() || !QStringList{"UserPromptSubmit","UserPromptQueued","TurnStarted"}.contains(event.value("type").toString()))continue;
            const auto body=event.value("detail").toString().trimmed();
            const double distance=qAbs(event.value("at").toDouble()-message.value("at").toDouble());
            if(event.value("source")!="hgs_voice" && event.value("attachments").toArray().isEmpty() && distance<=nearest && !submitted.isEmpty() && !body.isEmpty() && (body.startsWith(submitted.left(1000)) || submitted.startsWith(body.left(1000)))){match=i;nearest=distance;}
        }
        if(match>=0){auto event=sourceEvents[match].toObject();event["activity_key"]=eventKey(message);event["detail"]=message.value("detail");event["attachments"]=message.value("attachments");event["attachment_submitted_text"]=message.value("submitted_text");sourceEvents[match]=event;continue;}
        sourceEvents.append(message);
    }
    const QString fallbackPrompt = searching ? QString() : m_fallbackPrompt;
    const QString fg = m_dark ? "#e8edf4" : "#1a2733";
    const QString muted = m_dark ? "#9eabba" : "#657487";
    const QString accent = m_dark ? "#8bdfc0" : "#167357";
    const QString surface = m_dark ? "#242d36" : "#f3f6f8";
    const QString userSurface = m_dark ? "#243730" : "#edf6f1";
    const QString codeSurface = m_dark ? "#171e25" : "#e8eef2";
    const QString warning = m_dark ? "#edbd77" : "#9b6216";
    const QString warningSurface = m_dark ? "#2d2c23" : "#fff7e4";
    const QString blue = m_dark ? "#98bdeb" : "#366ba9";
    const QString violet = m_dark ? "#c5a8f5" : "#6c43b8";
    const QString noticeSurface = m_dark ? "#2a2536" : "#f4f0fb";
    // Thinking reads like a reply but stays quieter: no card fill, a pale bar
    // and dimmer text at the same size.
    const QString thinkingBar = m_dark ? "#4f6680" : "#aec3dc";
    const QString thinkingText = m_dark ? "#b9c2cd" : "#424a53";
    const QString thinkingLabel = m_dark ? "#8ea3bd" : "#4f6b8f";
    const QString border = m_dark ? "#34404a" : "#dbe3e9";
    // GitHub Markdown on the Zerus card and page colours. Code blocks and zebra
    // rows keep the journal's code surface so that they stay visible on the card.
    auto agentTheme = MarkdownTheme::github(m_dark, QColor(surface), m_scale);
    auto pageTheme = MarkdownTheme::github(m_dark, QColor(m_dark ? canvasDark(m_bubbles) : "#ffffff"), m_scale);
    agentTheme.subtle = pageTheme.subtle = QColor(codeSurface);
    // Dark: GitHub's zebra and border steps relative to the grey card instead of near-black
    // code rows, and denser inline code so that it stands out from the card.
    agentTheme.stripe = pageTheme.stripe = QColor(m_dark ? "#2a333d" : codeSurface);
    if (m_dark) {
        agentTheme.border = pageTheme.border = QColor("#3a444e");
        agentTheme.chip.setAlphaF(0.33f); pageTheme.chip.setAlphaF(0.33f);
    }
    QString html = QString("<html><head><style>body{color:%1;font-size:13px;}p{margin:6px 0;line-height:135%;}h1,h2,h3,h4{font-size:14px;margin:10px 0 6px;}pre{white-space:pre-wrap;}a{color:%2;text-decoration:none;}li{margin-bottom:4px;}</style></head><body>").arg(fg, accent);
    if (!details.value("task_journal").toBool())
        html += QString("<p style='font-size:10px;color:%1;margin-bottom:12px;'>%2</p>").arg(muted, searching ? tr("SEARCH RESULT — Saved message") : tr("RECORDED ACTIVITY — Messages and tool excerpts"));
    if (details.value("history_truncated").toBool())
        html += QString("<p style='color:%1;font-size:11px;'>%2</p>").arg(muted, tr("Older events have expired from the journal."));
    if (searching && m_searchResult.value("content_truncated").toBool())
        html += QString("<p style='color:%1;font-size:11px;'>%2</p>").arg(muted, tr("Excerpt around the match. This message is too long to display in full."));

    const auto attachmentCards=[&](const QJsonArray &files,const QString &owner){
        QString result;
        for(int i=0;i<files.size();++i) {
            const auto file=files[i].toObject();const auto name=file.value("name").toString();
            // Location metadata changes when a local submission becomes a
            // durable receipt. Keep its loaded thumbnail and card identity.
            const QJsonObject identity{{"name",file["name"]},{"mime",file["mime"]},{"bytes",file["bytes"]},{"reference",file["reference"]}};
            const auto key=QString::fromLatin1(QCryptographicHash::hash((m_sessionKey+'\n'+m_conversation+'\n'+owner+':'+QString::number(i)).toUtf8()+QJsonDocument(identity).toJson(QJsonDocument::Compact),QCryptographicHash::Sha256).toHex());
            const bool available=!file.value("request_id").toString().isEmpty() || !file.value("local_path").toString().isEmpty();
            if(available)m_attachmentLinks.insert(key,file);
            const auto size=file.value("bytes").toInteger();const auto meta=size>0?tr("%1 KiB").arg(qMax<qint64>(1,(size+1023)/1024)):tr("File");
            const auto reference=file.value("reference").toString();
            const auto caption=escaped((reference.isEmpty()?QString():reference+" ")+name)+(available?QString(" &nbsp; <span style='color:%1'>%2 &nbsp; %3</span>").arg(muted,escaped(meta),tr("Open →")):QString());
            QString preview;
            if(available && file.value("mime").toString().startsWith("image/") && file.value("mime")!="image/svg+xml") {
                m_previewFiles.insert(key,file);
                QImage placeholder(192,128,QImage::Format_ARGB32_Premultiplied);placeholder.fill(Qt::transparent);
                document->addResource(QTextDocument::ImageResource,QUrl("hgs-thumbnail:"+key), m_previews.value(key, placeholder));
                preview=QString("<td width='96' height='64'><a href='hgs-attachment:%1'><img src='hgs-thumbnail:%1' width='96' height='64'></a></td>").arg(key);
            }
            result+=QString("<table cellspacing='0' cellpadding='8' style='margin-top:8px;border:1px solid %1;'><tr>%2<td>%3</td></tr></table>").arg(border,preview,
                available?QString("<a href='hgs-attachment:%1' style='color:%2;'>%3</a>").arg(key,accent,caption):tr("Attachment: %1").arg(escaped(name)));
        }
        return result;
    };
    // WhiteWh fork: a preview under the message for each picture, animation or
    // video it names; the owner reads the file from the session's computer.
    const auto mediaTiles = [&](const QStringList &references) {
        QString result;
        for (const auto &href : references) {
            const auto key = QString::fromLatin1(QCryptographicHash::hash((m_sessionKey + '\n' + href).toUtf8(), QCryptographicHash::Sha256).toHex());
            m_mediaLinks.insert(key, href);
            const auto file = SessionFileReference::parse(href).path;
            const auto name = QFileInfo(file).fileName();
            const bool video = MediaFiles::kind(file) == MediaFiles::Kind::Video;
            document->addResource(QTextDocument::ImageResource, QUrl("hgs-media:" + key),
                                  m_mediaTiles.value(key, ActivityMedia::tile({}, name, video, tr("Loading…"))));
            result += QString("<table cellspacing='0' cellpadding='0' style='margin-top:8px;'><tr><td width='%2' height='%3'>"
                              "<a href='hgs-media:%1'><img src='hgs-media:%1' width='%2' height='%3'></a></td></tr></table>")
                          .arg(key).arg(ActivityMedia::Width).arg(ActivityMedia::Height);
        }
        return result;
    };
    // The header can carry recording metadata or a markdown copy button at its right.
    // A boundary marks a change that needs attention, such as a cleared context.
    const auto card = [&](const QString &key, const QString &label, const QString &stamp, const QString &body, bool user,
                          bool notice = false, bool thinking = false, const QString &markdownSource = {}, bool boundary = false,
                          const QString &headerMetadata = {}) {
        auto content = body;
        // User text always starts with a paragraph. Anchor inside that block:
        // Qt discards an empty anchor between paragraphs, and a header-relative
        // offset shifts when Sending changes to Submitted or Sent.
        if (user) content.insert(content.indexOf('>') + 1, QString("<a name='item-body-%1'></a>").arg(escaped(key)));
        const auto title = QString("<a name='item-%1'></a><b style='color:%2;'>%3</b><span style='color:%4;'>%5</span>")
            .arg(escaped(key), boundary ? warning : thinking ? thinkingLabel : notice ? violet : user ? accent : blue, escaped(label), muted,
                 stamp.isEmpty() ? QString() : QStringLiteral(" &nbsp;&nbsp; ") + escaped(stamp));
        auto header = "<p style='font-size:11px;margin-top:0;margin-bottom:8px;'>" + title + "</p>";
        if (!markdownSource.trimmed().isEmpty()) {
            const auto copyKey = QString::fromLatin1(QCryptographicHash::hash((m_sessionKey + '\n' + key).toUtf8(), QCryptographicHash::Sha256).toHex());
            m_copyTexts.insert(copyKey, markdownSource);
            // Qt drops the bottom margin of a cell's last paragraph: the table keeps the header spacing.
            header = QString("<table width='100%' cellspacing='0' cellpadding='0' style='margin-bottom:8px;'><tr><td><p style='font-size:11px;margin:0;'>%1</p></td>"
                "<td align='right' valign='top'><img src='%2' width='14' height='14'></td></tr></table>").arg(title, copyButton("copy", copyKey));
        } else if (!headerMetadata.isEmpty()) {
            header = QString("<table width='100%' cellspacing='0' cellpadding='0' style='margin-bottom:8px;'><tr><td valign='top'><p style='font-size:11px;margin:0;'>%1</p></td>"
                "<td align='right' valign='top'><p style='font-size:11px;color:%3;margin:0;'>%2</p></td></tr></table>").arg(title, headerMetadata, muted);
        }
        // WhiteWh fork: with bubbles, an agent reply reads as text in the column
        // under a quiet label line, without a card.
        if (m_bubbles && !user && !notice && !boundary && !thinking) {
            const auto quiet = QString("<a name='item-%1'></a><span style='color:%2;'>%3</span>")
                .arg(escaped(key), muted, escaped(stamp.isEmpty() ? label : stamp));
            auto line = "<p align='right' style='font-size:10px;margin-top:0;margin-bottom:2px;'>" + quiet + "</p>";
            if (!markdownSource.trimmed().isEmpty()) {
                const auto copyKey = QString::fromLatin1(QCryptographicHash::hash((m_sessionKey + '\n' + key).toUtf8(), QCryptographicHash::Sha256).toHex());
                m_copyTexts.insert(copyKey, markdownSource);
                line = QString("<table width='100%' cellspacing='0' cellpadding='0' style='margin-bottom:2px;'><tr><td></td><td align='right' valign='top'><p style='font-size:10px;margin:0;'>%1"
                    " &nbsp;<img src='%2' width='12' height='12'></p></td></tr></table>").arg(quiet, copyButton("copy", copyKey));
            }
            return QString("<p style='font-size:5px;margin:0;'>&nbsp;</p><table width='100%' cellspacing='0' cellpadding='0'><tr><td style='padding:6px 2px 4px;'>%1%2</td></tr></table>")
                .arg(line, content);
        }
        // The spacer goes before each card, so nothing pads the journal below the last one.
        if (m_bubbles && user && !notice && !boundary && !thinking) {
            // The bubble says who by its place; its line keeps the time and delivery.
            const QString bubble = m_dark ? "#1f3a31" : "#e0f0e9", bubbleBorder = m_dark ? "#2f5a4a" : "#9fd3bd";
            const bool plain = label == tr("You");
            const auto top = plain ? QString("<a name='item-%1'></a>").arg(escaped(key))
                : QString("<p style='font-size:10px;color:%1;margin:0 0 4px;'><a name='item-%2'></a>%3</p>").arg(accent, escaped(key), escaped(label));
            // The time and delivery stay under the bubble, small.
            const auto under = stamp.isEmpty() ? QString() : QString("<p align='right' style='font-size:10px;color:%1;margin:3px 2px 0;'>%2</p>").arg(muted, escaped(stamp));
            return QString("<p style='font-size:6px;margin:0;'>&nbsp;</p><table width='100%' cellspacing='0' cellpadding='0'><tr><td width='22%'></td><td>"
                           "<table width='100%' cellspacing='0' cellpadding='0' bgcolor='%1' style='border:1px solid %2;'><tr><td style='padding:9px 13px;'>%3%4</td></tr></table>%5</td></tr></table>")
                .arg(bubble, bubbleBorder, top, content, under);
        }
        // WhiteWh fork: with bubbles every other card is rounded too, its colour on the edge.
        if (m_bubbles)
            return QString("<p style='font-size:6px;margin:0;'>&nbsp;</p><table width='100%' cellspacing='0' cellpadding='0' bgcolor='%2' style='border:1px solid %5;'><tr><td width='3' bgcolor='%1'></td><td style='padding:10px 13px;'>"
                "%3%4</td></tr></table>")
                .arg(boundary ? warning : thinking ? thinkingBar : notice ? violet : user ? accent : blue,
                     boundary ? warningSurface : thinking ? pageTheme.canvas.name() : notice ? noticeSurface : user ? userSurface : surface, header, content,
                     m_dark ? "#2c343d" : "#e3e8ed");
        return QString("<p style='font-size:5px;margin:0;'>&nbsp;</p><table width='100%' cellspacing='0' cellpadding='0'><tr><td width='3' bgcolor='%1'></td><td bgcolor='%2' style='padding:11px 13px;'>"
            "%3%4</td></tr></table>")
            .arg(boundary ? warning : thinking ? thinkingBar : notice ? violet : user ? accent : blue,
                 boundary ? warningSurface : thinking ? pageTheme.canvas.name() : notice ? noticeSurface : user ? userSurface : surface, header, content);
    };

    QList<QJsonObject> events;
    QSet<QString> seen;
    bool voiceBoundPrompt = false;
    QList<QJsonObject> voiceBindings;
    for(const auto &value:sourceEvents) {
        const auto event=value.toObject();
        if(event.value("source")=="hgs_voice")voiceBindings.append(event.value("voice").toObject().value("native_binding").toObject());
    }
    for (const auto &value : sourceEvents) {
        if (!value.isObject()) continue;
        const auto event = value.toObject(); const auto key = eventKey(event);
        // Voice admission does not consume the native inbox. Keep its live
        // queue status and existing confirmed Remove alongside the recording.
        if(event.value("source")!="hgs_voice" && event.value("delivery_state")!="queued" && event.value("delivery_state")!="not_delivered") {
            bool bound=false;
            for(const auto &binding:voiceBindings) {
                const auto seq=binding.value("journal_seq").toInteger();
                const auto id=binding.value("provider_message_id").toString();
                const auto source=binding.value("provider_source").toString();
                if((seq>0 && event.value("seq").toInteger()==seq)
                    || (!id.isEmpty() && !source.isEmpty()
                        && event.value("provider_message_id").toString(event.value("message_id").toString())==id
                        && event.value("provider_source").toString(event.value("source").toString())==source)) {bound=true;break;}
            }
            if(bound) {
                if(!event.value("detail").toString().isEmpty() && event.value("detail")==details.value("prompt"))voiceBoundPrompt=true;
                continue;
            }
        }
        // Kimi emits this lifecycle hook without text before TurnStarted.
        // It still drives session state, but adds no content to the transcript.
        if (event.value("type") == "UserPromptSubmit" && event.value("agent_id").toString().isEmpty()
            && event.value("detail").toString().trimmed().isEmpty()
            && event.value("attachments").toArray().isEmpty()) continue;
        // Claude reminds a minute after every finished turn that it waits for input
        // (an empty Notification, or idle_prompt); approvals and questions have their own events.
        if (event.value("type") == "Notification" && (event.value("detail").toString().trimmed().isEmpty()
            || event.value("notification_type") == "idle_prompt")) continue;
        if (event.value("type") == "SessionCleared" && std::any_of(events.cbegin(), events.cend(),
                [&event](const QJsonObject &other) { return sameSessionClear(other, event); })) continue;
        if (!seen.contains(key)) { seen.insert(key); events.append(event); }
    }
    const bool sequenced = std::all_of(events.cbegin(), events.cend(), [](const QJsonObject &e) { return e.value("seq").toInteger() > 0; });
    const bool timed = std::all_of(events.cbegin(), events.cend(), [](const QJsonObject &e) { return e.value("at").toDouble() > 0; });
    std::stable_sort(events.begin(), events.end(), [sequenced, timed](const QJsonObject &a, const QJsonObject &b) {
        // A confirmed answer may be journaled after the next tool hook, but
        // retains the provider's actual answer time. Sequence is the cursor
        // and a stable tie-breaker, not the occurrence time of such events.
        if (timed && a.value("at") != b.value("at")) return a.value("at").toDouble() < b.value("at").toDouble();
        if (sequenced) return a.value("seq").toInteger() < b.value("seq").toInteger();
        return a.value("at").toDouble() < b.value("at").toDouble();
    });
    // Some providers include the same prompt in both submission and turn-start
    // hooks. Merge only adjacent acknowledgements; a repeated later request is
    // still a distinct message. Retain the submission key for scroll anchors.
    for (int i = 1; i < events.size();) {
        const auto &previous = events[i - 1];
        const auto &current = events[i];
        const auto previousType = previous.value("type").toString();
        const auto previousText=previous.value("attachment_submitted_text").toString(previous.value("detail").toString()).trimmed();
        const auto currentText=current.value("attachment_submitted_text").toString(current.value("detail").toString()).trimmed();
        if (previous.value("source") != "hgs_voice" && current.value("source") != "hgs_voice"
            && (previousType == "UserPromptSubmit" || previousType == "UserPromptQueued")
            && current.value("type") == "TurnStarted"
            && messageRole(previous) == "user" && messageRole(current) == "user"
            && (isExcerptOf(previousText, currentText)
                || isExcerptOf(currentText, previousText))) {
            if (!current.value("attachments").toArray().isEmpty()) {
                events[i - 1]["attachments"] = current.value("attachments");
                events[i - 1]["attachment_submitted_text"] = current.value("attachment_submitted_text");
                events[i - 1]["detail"] = current.value("detail");
            } else if (previous.value("attachments").toArray().isEmpty() && current.value("detail").toString().size() > previous.value("detail").toString().size())
                events[i - 1]["detail"] = current.value("detail");
            events.removeAt(i);
        } else ++i;
    }
    // WhiteWh fork: a cache keepalive is one quiet notice without its one-word
    // reply; keepalives in a row share it. Native histories keep the message text.
    for (int i = 0; i < events.size();) {
        const auto role = messageRole(events[i]);
        const bool keepalive = role == "notice" ? events[i].value("origin") == "cache_keepalive"
            : role == "user" && events[i].value("detail").toString().startsWith(QLatin1String("<zerus-cache-keepalive>"));
        if (!keepalive) { ++i; continue; }
        events[i]["origin"] = "cache_keepalive"; events[i]["detail"] = tr("Cache warmed");
        int next = i + 1;
        while (next < events.size() && messageRole(events[next]) == "assistant") events.removeAt(next);
        const bool joins = i > 0 && events[i - 1].value("origin") == "cache_keepalive";
        if (joins) {
            events[i - 1]["keepalive_count"] = events[i - 1].value("keepalive_count").toInt(1) + 1;
            events[i - 1]["at"] = events[i].value("at"); events.removeAt(i);
        } else ++i;
    }
    QString prompt = details.value("prompt").toString(fallbackPrompt);
    if(voiceBoundPrompt)prompt.clear();
    QString answer = details.value("last_message").toString();
    for (int i = events.size() - 1; i >= 0; --i) {
        const auto role = messageRole(events[i]);
        if (role == "user" && events[i].value("source") != "hgs_voice" && events[i].value("type") != "QuestionAnswered"
            && !prompt.isEmpty() && isExcerptOf(events[i].value("attachment_submitted_text").toString(events[i].value("detail").toString()).trimmed(), prompt.trimmed())) {
            if(events[i].value("attachments").toArray().isEmpty()) events[i]["detail"] = prompt;
            prompt.clear();
        } else if (role == "assistant" && !answer.isEmpty() && isExcerptOf(events[i].value("detail").toString(), answer)) {
            events[i]["detail"] = answer; answer.clear();
        }
    }
    // Snapshots without a matching event have no reliable position in the
    // timeline. Show them as preceding context, never as a new message after
    // the tools. Matched snapshots above stay at their actual event position.
    if (!prompt.isEmpty() || !answer.isEmpty()) {
        html += QString("<p style='font-size:10px;color:%1;margin:12px 0 8px;'>%2</p>")
            .arg(muted, tr("RECORDED CONTEXT — Outside the available timeline"));
        if (!prompt.isEmpty()) {
            const auto replies = QuestionReply::parse(prompt);
            html += card("prompt-snapshot", replies.isEmpty() ? tr("You (recorded request)") : tr("You (recorded answer)"), {},
                replies.isEmpty() ? userMessage(prompt) : questionReplyBody(replies, muted, accent, codeSurface, border), true);
        }
        if (!answer.isEmpty()) html += card("answer-snapshot", tr("Agent (recorded response)"), {}, markdown(answer, agentTheme, m_fileLinks), false, false, false, answer);
    }
    m_toggleKeys.clear();m_processLinks.clear();
    for (int i = 0; i < events.size();) {
        const auto event = events[i]; const auto role = messageRole(event);
        const bool taskEntry = event.value("source") == "zerus_task" && event.value("type") == "TaskEntry";
        if (taskEntry && event.value("task_role") == "document") {
            html += QString("<h3 style='color:%1;margin:16px 0 10px;'>%2</h3>")
                .arg(fg, escaped(event.value("task_label").toString()));
            html += markdown(event.value("detail").toString(), pageTheme, m_fileLinks);
            html += attachmentCards(event.value("attachments").toArray(), eventKey(event));
            if (event.value("task_editable").toBool()) html += QString("<p style='font-size:12px;margin:10px 0 20px;'><a href='hgs-task:edit' style='color:%1;'>%2</a></p>").arg(accent, escaped(tr("Edit description")));
            ++i; continue;
        }
        if (taskEntry && role.isEmpty()) {
            html += QString("<p style='font-size:11px;color:%1;margin:10px 0;'><span>%2</span> &nbsp; <b>%3</b> · %4</p>")
                .arg(muted, escaped(timeText(event)), escaped(event.value("task_author").toString()), escaped(event.value("detail").toString()));
            ++i; continue;
        }
        if (event.value("type") == "SessionCleared") {
            // Earlier activity stays readable above; the agent no longer has it.
            html += card(eventKey(event), tr("Session cleared"), timeText(event),
                QString("<p style='margin:0;color:%1;'>%2</p>").arg(warning,
                    tr("The agent no longer sees the activity above. New messages start with a fresh conversation context.")),
                false, false, false, {}, true);
            ++i; continue;
        }
        if (event.value("type") == "AgentThinking") {
            const auto text = event.value("detail").toString();
            if (!text.trimmed().isEmpty()) {
                auto thinkingTheme = pageTheme; thinkingTheme.fg = QColor(thinkingText);
                html += card(eventKey(event), tr("Thinking"), timeText(event), markdown(text, thinkingTheme, m_fileLinks), false, false, true);
            }
            ++i; continue;
        }
        if (role == "notice" && event.value("origin") == "cache_keepalive") {
            const int count = event.value("keepalive_count").toInt(1);
            const auto text = count > 1 ? tr("Cache warmed %1 times while idle").arg(count) : tr("Cache warmed while idle");
            html += QString("<p style='font-size:11px;color:%1;margin:6px 0;'>%2 · %3</p>").arg(muted, escaped(text), escaped(timeText(event)));
            ++i; continue;
        }
        if (role == "notice") {
            auto body = QString("<p style='margin:0;'>%1</p>").arg(escaped(event.value("detail").toString()).replace('\n', "<br>"));
            // Another session's message is meant to be read in full. A
            // subagent's report stays one line, like Claude's terminal, until opened.
            const auto origin = event.value("origin").toString();
            const auto report = event.value("report").toString();
            if (origin == "peer_message") {
                auto noticeTheme = MarkdownTheme::github(m_dark, QColor(noticeSurface), m_scale);
                noticeTheme.subtle = QColor(codeSurface);
                body = markdown(report.isEmpty() ? event.value("detail").toString() : report, noticeTheme, m_fileLinks);
            } else if (!report.isEmpty()) {
                const QString key = "report-" + eventKey(event); m_toggleKeys.insert(key);
                const bool expanded = m_expanded.value(key);
                body += QString("<p style='font-size:11px;margin:6px 0 0;'><a href='hgs-activity:%1' style='color:%2;'>%3</a></p>")
                    .arg(key, violet, expanded ? tr("▾ Hide report") : tr("▸ Show report"));
                if (expanded) {
                    auto noticeTheme = MarkdownTheme::github(m_dark, QColor(noticeSurface), m_scale);
                    noticeTheme.subtle = QColor(codeSurface);
                    body += markdown(report, noticeTheme, m_fileLinks);
                }
            }
            const QString label = origin == "subagent_report" ? tr("Subagent report")
                : origin == "peer_message" ? tr("Message from %1").arg(event.value("sender").toString()) : tr("Background task");
            html += card(eventKey(event), label, timeText(event), body, false, true);
            ++i; continue;
        }
        if (!role.isEmpty()) {
            const auto text = event.value("detail").toString();
            const auto replies = role == "user" ? QuestionReply::parse(text) : QJsonArray();
            if(event.value("source")=="hgs_voice") {
                const auto voice=event.value("voice").toObject();const auto video=voice.value("video").toObject();const auto audio=voice.value("audio").toObject();
                const auto id=voice.value("id").toString(event.value("message_id").toString());
                const auto key=QString::fromLatin1(QCryptographicHash::hash((m_sessionKey+"\n"+id).toUtf8(),QCryptographicHash::Sha256).toHex());
                if(!audio.value("request_id").toString().isEmpty())m_voiceLinks.insert(key,video.isEmpty()?audio:video);
                const auto state=voice.value("state").toString();
                const QHash<QString,QString> labels{{"uploading",tr("Sending recording…")},{"queued",tr("Waiting for recognition")},{"transcribing",tr("Recognizing…")},{"ready",tr("Ready to deliver")},
                    {"waiting_agent",tr("Waiting for agent")},{"delivering",tr("Delivering…")},{"submitted",tr("Accepted by native app")},{"failed",tr("Voice message failed")},
                    {"cancelled",tr("Cancelled")},{"uncertain",tr("Delivery not confirmed")}};
                const auto transcript=voice.value("transcript").toString();
                // Reserve the transcript button's width before laying out the
                // inline player, including when a scroll bar first appears.
                const int transcriptWidth=video.isEmpty() && !transcript.isEmpty()?40:0;
                const int playerWidth=qBound(80,int(qMin(width,widthWithScrollBar)/m_scale)-72-transcriptWidth,420);
                const QUrl playerUrl("hgs-voice-player:"+key);
                document->addResource(QTextDocument::ImageResource,playerUrl,
                    VoicePlayerImage::draw(qRound(playerWidth*m_scale),m_scale,document->pixelRatio,m_dark,audio,0,false,false));
                const auto label=tr("Play or pause voice message (%1)").arg(VoicePlayerImage::time(audio.value("duration_ms").toInteger()));
                QString player=QString("<a href='hgs-voice:%1' title='%2'><img src='%3' width='%4' height='52' style='vertical-align:middle;' alt='%2'></a>")
                    .arg(key,escaped(label),playerUrl.toString(),QString::number(playerWidth));
                if(!video.isEmpty()){
                    document->addResource(QTextDocument::ImageResource,playerUrl,VoicePlayerImage::videoPoster(m_scale,document->pixelRatio,video));
                    player=QString("<a href='hgs-voice:%1' title='%2'><img src='%3' width='200' height='200' alt='%2'></a>")
                        .arg(key,escaped(tr("Play video message")),playerUrl.toString());
                }
                QString transcriptAction;
                if(!transcript.isEmpty()) {
                    const auto toggle="voice-text-"+key;m_toggleKeys.insert(toggle);
                    const QUrl icon("hgs-voice-transcript:"+key);
                    document->addResource(QTextDocument::ImageResource,icon,VoicePlayerImage::transcriptIcon(m_scale,document->pixelRatio,m_dark,m_expanded.value(toggle)));
                    const auto label=escaped(m_expanded.value(toggle)?tr("Hide transcript"):tr("Show transcript"));
                    transcriptAction=QString("<a href='hgs-activity:%1' title='%2'><img src='%3' width='32' height='32' style='vertical-align:middle;' alt='%2'></a>").arg(toggle,label,icon.toString());
                }
                QString body=video.isEmpty()
                    ?QString("<p style='margin:8px 0 4px;'>%1%2</p>").arg(player,transcriptAction.isEmpty()?QString():"&nbsp;&nbsp;"+transcriptAction)
                    :QString("<p style='margin:4px 0;'>%1</p>").arg(player);
                const auto statusLabel=escaped(labels.value(state,tr("Delivery not confirmed")));
                const auto statusDetail=state=="submitted"?escaped(tr("The native app accepted the recognized text. Queue status, when present, is shown alongside this recording. This does not confirm model delivery, reading or completion.")):statusLabel;
                const QUrl statusIcon("hgs-voice-status:"+key);
                document->addResource(QTextDocument::ImageResource,statusIcon,VoicePlayerImage::deliveryIcon(state,m_scale,document->pixelRatio,m_dark));
                const auto mediaTime=video.isEmpty()?QString():VoicePlayerImage::time(video.value("duration_ms").toInteger());
                if(!video.isEmpty())body+=QString("<p style='margin:0;font-size:11px;color:%3;'>%1%2</p>").arg(transcriptAction,escaped(mediaTime),muted);
                const auto metadata=QString("%1 &nbsp;<img src='%2' width='18' height='18' style='vertical-align:middle;' alt='%3' title='%4'>")
                    .arg(escaped(timeText(event)),statusIcon.toString(),statusLabel,statusDetail);
                if(!transcript.isEmpty() && m_expanded.value("voice-text-"+key))body+=userMessage(transcript);
                if(!voice.value("error").toString().isEmpty())body+=QString("<p style='color:%1;'>%2</p>").arg(warning,escaped(voice.value("error").toString()));
                if(!m_voiceError.isEmpty() && m_voiceError.startsWith(key+"\n"))body+=QString("<p style='color:%1;'>%2</p>").arg(warning,escaped(m_voiceError.mid(key.size()+1)));
                html+=card(eventKey(event),video.isEmpty()?tr("You (voice)"):tr("You (video)"),{},body,true,false,false,{},false,metadata);
                ++i;continue;
            }
            // WhiteWh fork: agent teams. A team message reads as a plaque.
            if (role == "user" && !taskEntry && replies.isEmpty() && event.value("type") != "QuestionAnswered") {
                if (const auto team = teamMessage(text); team.valid) {
                    const QString edge = teamRoleColor(team.from, m_dark);
                    const auto chip = [&](const QString &content, const QString &color = {}) {
                        return QString("<span style='color:%1;'>&nbsp;%2&nbsp;</span>&nbsp;").arg(color.isEmpty() ? muted : color, content);
                    };
                    // WhiteWh fork: with bubbles the header's chips are rounded pills.
                    const QString pillFill = m_dark ? "#262d36" : "#edf1f4";
                    const auto pill = [&](const QString &content) {
                        return QString("<td style='padding-right:5px;'><table cellspacing='0' cellpadding='0' bgcolor='%1'><tr><td style='padding:1px 7px;'><p style='font-size:11px;margin:0;white-space:nowrap;'>%2</p></td></tr></table></td>")
                            .arg(pillFill, content);
                    };
                    QString chips = QString("<b style='color:%1;'>%2</b>&nbsp;&nbsp;").arg(fg, escaped(team.kind));
                    chips += chip(escaped(team.to.isEmpty() ? team.from : team.from + QStringLiteral(" \u2192 ") + team.to));
                    if (team.task) chips += QString("<a href='hgs-team-task:%1' style='color:%2;'>%3</a>&nbsp;&nbsp;").arg(team.task).arg(accent,
                        escaped(team.title.isEmpty() ? tr("Task #%1").arg(team.task) : tr("Task: %1").arg(team.title)));
                    if (team.attempt) chips += chip(escaped(tr("Attempt %1").arg(team.attempt)));
                    const auto toggle = "team-plaque-" + eventKey(event);
                    m_toggleKeys.insert(toggle);
                    const bool open = m_expanded.value(toggle);
                    // Three lines unless opened: long hand-offs keep the journal readable.
                    QString body = open ? team.full : team.body;
                    const auto lines = body.split('\n');
                    bool clipped = false;
                    if (!open && (lines.size() > 3 || body.size() > 320)) {
                        body = lines.mid(0, 3).join('\n').left(320).trimmed() + QStringLiteral(" \u2026"); clipped = true;
                    }
                    if (!open && team.body != team.full) clipped = true;
                    QString more = clipped || open ? QString("<a href='hgs-activity:%1' style='color:%2;'>%3</a>").arg(toggle, accent, open ? tr("Show less") : tr("Show more")) : QString();
                    auto header = QString("<table width='100%' cellspacing='0' cellpadding='0' style='margin-bottom:6px;'><tr><td><p style='font-size:11px;margin:0;'>%1</p></td>"
                        "<td align='right' valign='top'><p style='font-size:11px;color:%2;margin:0;'>%3</p></td></tr></table>").arg(chips, muted, escaped(timeText(event)));
                    if (m_bubbles) {
                        QString cells = QString("<td style='padding-right:8px;'><p style='font-size:12px;margin:0;white-space:nowrap;'><b style='color:%1;'>%2</b></p></td>").arg(fg, escaped(team.kind));
                        cells += pill(QString("<span style='color:%1;'>%2</span>").arg(muted, escaped(team.to.isEmpty() ? team.from : team.from + QStringLiteral(" → ") + team.to)));
                        if (team.task) cells += pill(QString("<a href='hgs-team-task:%1' style='color:%2;'>%3</a>").arg(team.task).arg(accent,
                            escaped(team.title.isEmpty() ? tr("Task #%1").arg(team.task) : tr("Task: %1").arg(team.title))));
                        if (team.attempt) cells += pill(QString("<span style='color:%1;'>%2</span>").arg(muted, escaped(tr("Attempt %1").arg(team.attempt))));
                        header = QString("<table width='100%' cellspacing='0' cellpadding='0' style='margin-bottom:6px;'><tr>%1<td width='100%'></td>"
                            "<td align='right' valign='middle'><p style='font-size:11px;color:%2;margin:0;white-space:nowrap;'>%3</p></td></tr></table>").arg(cells, muted, escaped(timeText(event)));
                    }
                    const auto footer = QString("<table width='100%' cellspacing='0' cellpadding='0' style='margin-top:6px;'><tr><td><p style='font-size:10px;color:%1;margin:0;font-family:%4;'>%2</p></td>"
                        "<td align='right'><p style='font-size:11px;margin:0;'>%3</p></td></tr></table>").arg(muted, escaped(team.header), more, MarkdownHtml::monospaceFamily());
                    if (m_bubbles) {
                        // A rounded card: the background moves to the table, a divider tops the footer.
                        const QString card = m_dark ? "#20272f" : "#ffffff", line = m_dark ? "#2c343d" : "#e3e8ed";
                        const auto divided = QString("<table width='100%' cellspacing='0' cellpadding='0' style='margin-top:8px;'><tr><td bgcolor='%1'><p style='font-size:1px;line-height:1px;margin:0;'>&nbsp;</p></td></tr></table>").arg(line) + footer;
                        html += QString("<a name='item-%1'></a><p style='font-size:6px;margin:0;'>&nbsp;</p><table width='100%' cellspacing='0' cellpadding='0' bgcolor='%4' style='border:1px solid %2;'><tr><td width='3' bgcolor='%3'></td>"
                            "<td style='padding:10px 12px;'>%5<p style='white-space:pre-wrap;margin:0;color:%6;'>%7</p>%8</td></tr></table>")
                            .arg(escaped(eventKey(event)), line, edge, card, header, fg, escaped(body), divided);
                    } else
                    html += QString("<a name='item-%1'></a><p style='font-size:5px;margin:0;'>&nbsp;</p><table width='100%' cellspacing='0' cellpadding='0' style='border:1px solid %2;'><tr><td width='3' bgcolor='%3'></td>"
                        "<td bgcolor='%4' style='padding:10px 12px;'>%5<p style='white-space:pre-wrap;margin:0;color:%6;'>%7</p>%8</td></tr></table>")
                        .arg(escaped(eventKey(event)), border, edge, surface, header, fg, escaped(body), footer);
                    ++i; continue;
                }
            }
            const QString label = taskEntry ? event.value("task_author").toString() + QStringLiteral(" · ") + event.value("task_label").toString()
                : role == "user" ? (event.value("type") == "UserPromptQueued" ? tr("You (queued)") :
                event.value("type") == "QuestionAnswered" || !replies.isEmpty() ? tr("You (answer)") : tr("You")) : tr("Agent");
            auto stamp=timeText(event);
            if (taskEntry && !event.value("task_delivery").toString().isEmpty()) stamp += "  " + event.value("task_delivery").toString();
            // OpenCode reports its inbox: a waiting message is queued, and after
            // Stop it stays there, not delivered, until it is sent or removed.
            const auto delivery=role=="user"?event.value("delivery_state").toString():QString();
            const bool inbox=delivery=="queued"||delivery=="not_delivered";
            if (inbox) stamp += "  " + (delivery == "queued" ? tr("Queued") : tr("Not delivered"));
            else if (role == "user" && event["type"] != "LocalMessage" && !taskEntry) {
                bool accepted = event["type"] != "UserPromptQueued";
                if (!accepted) for (int j=i+1;j<events.size();++j) {
                    const auto next = events[j];
                    if ((next["type"]=="UserPromptSubmit" || next["type"]=="TurnStarted") && next["detail"]==event["detail"]) { accepted=true; break; }
                }
                stamp += "  " + (accepted ? tr("Sent") : tr("Queued at this time"));
            }
            QStringList media;
            auto body = role == "user" ? (replies.isEmpty() ? userMessage(text) : questionReplyBody(replies, muted, accent, codeSurface, border))
                                       : markdown(text, agentTheme, m_fileLinks, &media);
            if (taskEntry && role == "user") {
                auto theme = MarkdownTheme::github(m_dark, QColor(userSurface), m_scale); theme.subtle = QColor(codeSurface);
                body = markdown(text, theme, m_fileLinks);
            }
            if(event.value("type")=="LocalMessage") {
                const auto state=event.value("status").toString();stamp=state=="sending"?tr("Sending…"):state=="error"?tr("Not sent"):tr("Submitted to terminal");
                if(state=="error" && event.value("uncertain").toBool())stamp=tr("Delivery not confirmed");
                if(state=="error")body+=QString("<p style='font-size:11px;color:%1;'>%2</p>").arg(warning,escaped(event.value("error").toString()));
            }
            body+=attachmentCards(event.value("attachments").toArray(),eventKey(event));
            body+=mediaTiles(media);
            if(event.value("type")=="LocalMessage" && event.value("status")=="error" && !event.value("id").toString().isEmpty()) {
                const auto button=[&](const QString &action,const QString &label) {
                    const auto key=QString::fromLatin1(QCryptographicHash::hash((m_sessionKey+':'+event.value("id").toString()+':'+action).toUtf8(),QCryptographicHash::Sha256).toHex());
                    m_messageActions.insert(key,{event.value("id").toString(),action});
                    return QString("<td bgcolor='%1' style='border:1px solid %2;'><a href='hgs-message:%3' style='color:%4;'>&nbsp;%5&nbsp;</a></td>")
                        .arg(codeSurface,border,key,accent,escaped(label));
                };
                body+=QString("<table cellspacing='6' cellpadding='4'><tr>%1%2</tr></table>")
                    .arg(event.value("uncertain").toBool()?button("inspect",details.value("native_ui_available").toBool()?tr("Check native UI"):tr("Check Terminal")):button("retry",tr("Retry")),button("delete",tr("Delete")));
            }
            // Remove is offered only while the session reports it; the owner confirms first.
            // While one removal runs, no Remove can be clicked.
            const auto queuedId=event.value("message_id").toString();
            // Not delivered is not a refusal: OpenCode keeps the message queued and
            // runs it before the next one; Zerus never resends it.
            if(delivery=="not_delivered")
                body+=QString("<p style='font-size:11px;color:%1;margin:6px 0 0;'>%2</p>").arg(muted,
                    escaped(tr("Still waiting in OpenCode's queue. It runs first with your next message unless you remove it.")));
            if(delivery=="not_delivered" && !queuedId.isEmpty() && details.value("allowed_actions").toArray().contains(QStringLiteral("remove_queued"))) {
                const auto removing=details.value("queued_removal").toString();
                if(!removing.isEmpty() || details.value("queued_removal_busy").toBool())
                    body+=QString("<table cellspacing='6' cellpadding='4'><tr><td bgcolor='%1' style='border:1px solid %2;color:%3;'>&nbsp;%4&nbsp;</td></tr></table>")
                        .arg(codeSurface,border,muted,escaped(removing==queuedId?tr("Removing…"):tr("Remove")));
                else {
                    const auto key=QString::fromLatin1(QCryptographicHash::hash((m_sessionKey+':'+queuedId+":remove_queued").toUtf8(),QCryptographicHash::Sha256).toHex());
                    m_messageActions.insert(key,{queuedId,QStringLiteral("remove_queued")});
                    body+=QString("<table cellspacing='6' cellpadding='4'><tr><td bgcolor='%1' style='border:1px solid %2;'><a href='hgs-message:%3' style='color:%4;'>&nbsp;%5&nbsp;</a></td></tr></table>")
                        .arg(codeSurface,border,key,accent,escaped(tr("Remove")));
                }
            }
            // A search excerpt is not the whole reply.
            const bool copyable = role == "assistant" && !(searching && m_searchResult.value("content_truncated").toBool());
            html += card(eventKey(event), label, stamp, body, role == "user", false, false, copyable ? text : QString());
            ++i; continue;
        }
        // Collapse adjacent operational events together. Message and attention
        // boundaries always remain visible; tools from separate turns do not mix.
        const bool thinking=event.value("type")=="AgentThinking";
        QList<QJsonObject> group{event}; ++i;
        while (!thinking && i < events.size() && events[i].value("type")!="AgentThinking" && events[i].value("type")!="SessionCleared" && group.size() < 24 && messageRole(events[i]).isEmpty()
               && !attention(event) && !attention(events[i])) group.append(events[i++]);
        const QString key = "group-" + eventKey(event); m_toggleKeys.insert(key);
        if (!m_expanded.contains(key) && attention(event)) m_expanded.insert(key, true);
        const bool expanded = m_expanded.value(key);
        QStringList tools;
        int calls = 0;
        for (const auto &e : group) {
            const QString name = e.value("tool").toString();
            if (!name.isEmpty() && !tools.contains(name)) tools.append(name);
            if (e.value("type") == "PreToolUse") ++calls;
        }
        const QString callLabel = calls == 1 ? tr("1 tool call") : tr("%1 tool calls").arg(calls);
        QString title = thinking ? tr("Thinking") : group.size() == 1 ? eventTitle(event.value("type").toString()) :
            calls > 0 ? tr("%1, %2 events").arg(callLabel).arg(group.size()) : tr("%1 activity events").arg(group.size());
        if (!tools.isEmpty()) title += QStringLiteral(" / ") + tools.mid(0, 3).join(", ") + (tools.size() > 3 ? QStringLiteral(" …") : QString());
        const QString color = attention(event) ? warning : muted;
        const auto last = group.last();
        QString preview = last.value("detail").toString().simplified();
        if (preview.size() > 110) preview = preview.left(109) + QChar(0x2026);
        if (m_bubbles) {
            // WhiteWh fork: each tool call is a quiet row on a thin rule: a glyph,
            // the verb and what it worked on. The verb opens the group's details.
            const auto verb = [&](const QString &tool) -> std::pair<QString, QString> {
                const auto t = tool.toLower();
                if (t == "read" || t == "view" || t == "read_file") return {tr("Read"), "read"};
                if (t.contains("edit") || t == "write" || t == "apply_patch" || t == "patch" || t == "create") return {tr("Edited"), "edit"};
                if (t == "grep" || t == "glob" || t == "find" || t.contains("search")) return {tr("Searched"), "search"};
                if (t == "bash" || t == "shell" || t == "exec" || t == "command" || t == "run" || t == "powershell") return {tr("Ran"), "run"};
                return {tool, "run"};
            };
            QList<QJsonObject> rows;
            if (!thinking) for (const auto &e : group) if (e.value("type") == "PreToolUse") rows.append(e);
            if (rows.isEmpty()) rows.append(event);
            const QColor glyphColor(attention(event) ? warning : muted);
            const int shown = qMin(int(rows.size()), 6);
            html += QString("<a name='item-%1'></a><p style='font-size:4px;margin:0;'>&nbsp;</p>").arg(escaped(key));
            for (int r = 0; r < shown; ++r) {
                const auto &e = rows[r];
                const auto [word, kind] = thinking ? std::pair<QString, QString>{tr("Thinking"), QStringLiteral("read")}
                                                   : e.value("tool").toString().isEmpty() ? std::pair<QString, QString>{title, QStringLiteral("run")} : verb(e.value("tool").toString());
                const QString glyph = QStringLiteral("hgs-tool:%1/%2").arg(kind, glyphColor.name().mid(1));
                if (document->resource(QTextDocument::ImageResource, QUrl(glyph)).value<QImage>().isNull())
                    document->addResource(QTextDocument::ImageResource, QUrl(glyph), toolGlyph(kind, glyphColor, document->pixelRatio));
                QString what = thinking ? QString() : e.value("detail").toString().simplified();
                if (what.size() > 110) what = what.left(109) + QChar(0x2026);
                const QString more = r == shown - 1 && rows.size() > shown ? tr("  · %1 more").arg(rows.size() - shown) : QString();
                html += QString("<table width='100%' cellspacing='0' cellpadding='0' style='margin-top:2px;'><tr><td width='2' bgcolor='%1'></td>"
                    "<td style='padding:3px 10px;'><img src='%2' width='14' height='14' style='vertical-align:middle;'>&nbsp;&nbsp;<a href='hgs-activity:%3' style='color:%4;text-decoration:none;'><b>%5</b></a>"
                    "<span style='color:%6;'>&nbsp;&nbsp;%7%8</span></td></tr></table>")
                    .arg(border, glyph, key, attention(event) ? warning : fg, escaped(word), muted, escaped(what), escaped(more));
            }
            html += "<table width='100%' cellspacing='0' cellpadding='0'><tr><td>";
        } else {
            html += QString("<p style='font-size:5px;margin:0;'>&nbsp;</p><table width='100%' cellspacing='0' cellpadding='9' style='border:1px solid %1;'><tr><td><a name='item-%2'></a>"
                "<a href='hgs-activity:%3' style='color:%4;'><b>%5 %6</b></a><span style='font-size:10px;color:%7;'> &nbsp; %8</span>")
                .arg(border, escaped(key), key, color, expanded ? QStringLiteral("▾") : QStringLiteral("▸"), escaped(title), muted, escaped(timeText(last)));
            if (!expanded && !preview.isEmpty()) html += QString("<p style='color:%1;font-size:11px;margin:4px 0 0;'>%2</p>").arg(muted, escaped(preview));
        }
        // WhiteWh fork: the pictures these tools made or read, open or closed.
        if (!thinking) html += mediaTiles(toolMedia(group));
        if(expanded && thinking)html+=markdown(event.value("detail").toString(),pageTheme,m_fileLinks);
        else if (expanded) for (const auto &e : group) {
            QString label = eventTitle(e.value("type").toString());
            if (!e.value("tool").toString().isEmpty()) label += QStringLiteral(" / ") + e.value("tool").toString();
            if (!e.value("agent_id").toString().isEmpty()) label += tr(" (subagent %1)").arg(e.value("agent_id").toString());
            html += QString("<p style='font-size:11px;color:%1;margin:9px 0 4px;'><b>%2</b> &nbsp; %3</p>").arg(muted, escaped(label), escaped(timeText(e)));
            const auto process=e.value("process_id").toString();
            bool recordedProcess=false;
            for(const auto &job:m_details.value("processes").toObject().value("items").toArray())if(job.toObject().value("id").toString()==process){recordedProcess=true;break;}
            if(processesEnabled && recordedProcess && QRegularExpression("^[a-f0-9]{32}$").match(process).hasMatch()){
                m_processLinks.insert(process);
                html+=QString("<a href='hgs-process:%1' style='color:%2;'>%3</a>").arg(process,accent,tr("View process"));
            }
            const auto detail = e.value("detail").toString();
            if (!detail.isEmpty()) html += QString("<p style=\"font-family:'%1';font-size:12px;background:%2;padding:7px;\">%3</p>")
                .arg(MarkdownHtml::monospaceFamily(), codeSurface, escaped(detail).replace('\n', "<br>"));
        }
        html += "</td></tr></table>";
    }
    const bool hasContent = !events.isEmpty() || !prompt.isEmpty() || !answer.isEmpty() || !localMessages.isEmpty();
    if (!hasContent) {
        const bool nativeSession = m_details.value("backend") == "dsh" && m_details.value("runtime_state") == "live" && !m_details.value("read_only").toBool();
        const bool signIn = nativeSession && m_details.value("auth_required").toBool();
        const bool nativeReady = nativeSession && m_details.value("activity") == "idle";
        html += QString("<p style='margin-top:18px;'><b>%1</b></p><p style='color:%2;'>%3</p>")
            .arg(signIn ? tr("No messages yet") : nativeReady ? tr("Ready for your first message") : tr("Activity will appear here"), muted,
                signIn ? tr("After signing in, send a message below to start this session.") :
                nativeReady ? tr("Send a message below. The agent's replies, tools and approvals will appear here.") :
                m_tracked ? tr("Messages, tools and approvals will appear as your agent works.") : tr("This session has no recorded activity. Open its terminal to see the agent."));
    }
    html += "</body></html>";
    m_html = ContentScale::html(html, m_scale);
    m_knownEvents = seen;
    content = hasContent;
    document->setUndoRedoEnabled(false);
    QTextCursor edit(document.get()); edit.beginEditBlock();
    document->setHtml(m_html);
    MarkdownObjects::convertChips(document.get(), agentTheme);
    edit.endEditBlock();
    if (m_bubbles) roundCards(document.get());
    for(auto block=document->begin();block.isValid();block=block.next())for(auto it=block.begin();!it.atEnd();++it){
        const auto fragment=it.fragment();const QUrl image(fragment.charFormat().toImageFormat().name());
        if(image.scheme()=="hgs-voice-player")m_voicePositions.insert(image.path(),fragment.position());
    }
    document->setTextWidth(width);
    size = document->documentLayout()->documentSize();
    if (size.height() > viewportHeight && widthWithScrollBar != width) {
        width = widthWithScrollBar;
        document->setTextWidth(width);
        size = document->documentLayout()->documentSize();
    }
    anchors = bookmarks(document.get());

    }
};

namespace {
void destroyRenderLifetime();
struct RenderJob {
    std::shared_ptr<ActivityRenderResult> result = std::make_shared<ActivityRenderResult>();
    QSemaphore prepared;
    QPointer<JournalDocument> document;
    QPointer<QTextBrowser> browser;
};
class RenderLifetime final : public QObject {
public:
    explicit RenderLifetime(QObject *parent) : QObject(parent) {}
    ~RenderLifetime() override {
        qRemovePostRoutine(destroyRenderLifetime);
        // FreeType faces belong to their preparing thread even after the Qt
        // document moves. Finish preparation, destroy every document tree while
        // those faces remain alive, then stop the parked threads. This drain is
        // only for application shutdown; navigation never waits for a worker.
        for (const auto &job : jobs) job->prepared.acquire();
        for (const auto &job : jobs) {
            auto *document = job->document.data();
            if (!document) continue;
            if (job->browser && job->browser->document() == document) {
                auto *empty = new JournalDocument(job->browser);
                empty->setDefaultFont(job->browser->font());
                job->browser->setDocument(empty);
            }
            if (job->result->document.get() == document) job->result->document.reset();
            else delete document;
        }
        for (auto *worker : jobs.keys()) worker->quit();
        for (auto *worker : jobs.keys()) worker->wait();
        jobs.clear();
    }
    QHash<QThread *, std::shared_ptr<RenderJob>> jobs;
};
QPointer<RenderLifetime> renderOwner;
QPointer<QCoreApplication> renderClosingApp;
void destroyRenderLifetime()
{
    // QApplication calls post routines before deleting widget/font/palette
    // globals. QObject-child destruction happens after those globals are gone.
    // Deleting the receiver also removes any queued finished callbacks before
    // their captured result documents and worker pointers can become stale.
    renderClosingApp = qApp;
    auto *owner = renderOwner.data();
    renderOwner.clear();
    delete owner;
}
RenderLifetime *renderLifetime()
{
    if (!renderOwner) {
        renderOwner = new RenderLifetime(qApp);
        qAddPostRoutine(destroyRenderLifetime);
        QObject::connect(qApp, &QCoreApplication::aboutToQuit, renderOwner, [] { destroyRenderLifetime(); });
    }
    return renderOwner;
}
}

ActivityView::ActivityView(QWidget *parent) : QWidget(parent)
{
    setObjectName("activityView");
    auto *layout = new QVBoxLayout(this); layout->setContentsMargins(0, 0, 0, 0); layout->setSpacing(5);
    auto *browser = new ActivityBrowser(this); m_browser = browser; m_browser->setObjectName("activity");
    m_browser->setDocument(new JournalDocument(m_browser));
    m_browser->setFrameShape(QFrame::NoFrame);
    m_browser->setOpenLinks(false); m_browser->setOpenExternalLinks(false);
    m_browser->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_browser->setWordWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
    m_browser->setAccessibleName(tr("Session activity"));
    layout->addWidget(m_browser, 1);
    // A browser child overlays the viewport without participating in layout
    // or moving with its scrolled contents.
    m_latest = new QPushButton(m_browser); m_latest->setObjectName("activityJumpLatest");
    m_latest->setAccessibleName(tr("Jump to latest activity"));
    m_latest->hide();
    m_fullWidth = new QAction(tr("Full-width Activity"), this); m_fullWidth->setCheckable(true);
    connect(m_fullWidth, &QAction::triggered, this, &ActivityView::fullWidthRequested);
    m_resetWidth = new QAction(tr("Reset Activity width"), this);
    connect(m_resetWidth, &QAction::triggered, this, &ActivityView::columnResetRequested);
    browser->extendMenu = [this](QMenu *menu) { addColumnActions(menu); };
    for (const bool right : {false, true}) {
        auto *edge = new ColumnEdge(right, m_browser);
        edge->pressed = [this] { m_dragOrigin = m_browser->viewport()->width(); };
        edge->dragged = [this](int distance) { dragColumn(distance); };
        edge->reset = [this] { emit columnResetRequested(); };
        edge->menuRequested = [this](const QPoint &position) { showColumnMenu(position); };
        m_edges.append(edge);
    }
    // Viewport events need the column edges.
    m_browser->viewport()->installEventFilter(this);
    m_browser->installEventFilter(this);
    m_compaction = new QWidget(this); m_compaction->setObjectName("activityCompaction");
    m_compaction->setAttribute(Qt::WA_StyledBackground);
    auto *progressRow = new QHBoxLayout(m_compaction); progressRow->setContentsMargins(12, 3, 12, 3); progressRow->setSpacing(10);
    auto *progress = new CompactionProgress(m_compaction); progress->setObjectName("compactionProgress");
    progress->setRange(0, 0); progress->setTextVisible(false); progress->setFixedSize(48, 5);
    progress->setAccessibleName(tr("Compacting context"));
    auto *caption = new QLabel(tr("Compacting context…"), m_compaction); caption->setObjectName("compactionCaption");
    caption->setTextFormat(Qt::PlainText);
    progressRow->addWidget(progress); progressRow->addWidget(caption); progressRow->addStretch();
    m_compaction->setToolTip(tr("The agent is compressing conversation context."));
    layout->addWidget(m_compaction); m_compaction->hide();
    m_queue = new QWidget(this); m_queue->setObjectName("activityInputQueue"); m_queue->setAttribute(Qt::WA_StyledBackground);
    auto *queueLayout = new QVBoxLayout(m_queue); queueLayout->setContentsMargins(12,8,12,8);
    auto *queueHeader = new QHBoxLayout;
    auto *queueCaption = new QLabel(tr("Queued in agent")); queueCaption->setObjectName("queueCaption");
    queueHeader->addWidget(queueCaption); queueHeader->addStretch();
    m_queueSend = new QPushButton(tr("Send now")); m_queueSend->setObjectName("queueSendNow"); queueHeader->addWidget(m_queueSend);
    queueLayout->addLayout(queueHeader);
    m_queueText = new QLabel; m_queueText->setObjectName("queueText"); m_queueText->setTextFormat(Qt::PlainText); m_queueText->setWordWrap(true);
    m_queueText->setTextInteractionFlags(Qt::TextSelectableByMouse); m_queueText->setMaximumHeight(100);
    queueLayout->addWidget(m_queueText);
    m_queueColumn = new ActivityWidth::ColumnLayout; layout->addLayout(m_queueColumn);
    m_queueColumn->addWidget(m_queue); m_queue->hide();
    connect(m_queueSend, &QPushButton::clicked, this, [this] { emit queueSendNowRequested(m_details["input_queue"].toObject()["id"].toString()); });
    connect(m_latest, &QPushButton::clicked, this, &ActivityView::jumpToLatest);
    connect(m_browser, &QTextBrowser::anchorClicked, this, &ActivityView::activateLink);
    connect(this, &ActivityView::externalLinkActivated, this, [](const QUrl &url) { QDesktopServices::openUrl(url); });
    connect(m_browser->verticalScrollBar(), &QScrollBar::valueChanged, this, [this] {
        if (m_rendering) return;
        m_followLatest = nearBottom() && !m_browser->textCursor().hasSelection();
        if (m_followLatest) m_unseen = 0;
        updateJumpButton();
    });
    connect(m_browser->verticalScrollBar(), &QScrollBar::rangeChanged, this, [this] {
        if (!m_rendering && m_followLatest && !m_browser->textCursor().hasSelection()) scheduleFollow();
    });
    connect(m_browser, &QTextBrowser::selectionChanged, this, [this] {
        if (!m_rendering && m_browser->textCursor().hasSelection()) m_followLatest = false;
    });
    setTheme(false); updateColumnActions();
}

void ActivityView::setSessionKey(const QString &key)
{
    resetSessionKey(key, false);
}

void ActivityView::resetSessionKey(const QString &key, bool preserveDocument)
{
    if (key == m_sessionKey) return;
    if(m_videoDialog)m_videoDialog->close();
    if(m_voicePlayer)m_voicePlayer->stop();
    m_voiceKey.clear();m_voicePlaying.clear();m_voiceLoading.clear();m_voiceError.clear();m_voiceLinks.clear();m_voicePositions.clear();
    m_voicePressed.clear();m_voicePosition=0;m_voicePendingSeek=-1;m_voiceStartAfterLoad=false;
    m_sessionKey = key; m_conversation.clear(); m_expanded.clear(); m_knownEvents.clear();
    m_attachmentLinks.clear();m_fileLinks.clear();
    m_previewFiles.clear(); m_previews.clear(); m_deliveryKeys.clear(); m_attachmentBindings.clear();
    clearMedia();
    m_messageActions.clear(); m_copyTexts.clear();
    m_localMessages = {}; m_searchResult = {}; m_searchQuery.clear(); m_browser->setExtraSelections({});
    m_initial = true; m_followLatest = true; m_unseen = 0; m_html.clear(); m_activityInput = {};
    ++m_renderGeneration; m_renderDirty = false; m_pendingContentUpdate = false;
    m_details = {}; m_events = {}; m_fallbackPrompt.clear();
    m_taskJournalReset = preserveDocument;
    if (!preserveDocument) clearDocument();
    updateJumpButton();
    m_compaction->hide(); m_queue->hide();
}

void ActivityView::continueSession(const QString &oldKey, const QString &newKey)
{
    if (m_sessionKey != oldKey || newKey.isEmpty() || oldKey == newKey) return;
    m_sessionKey = newKey;
    // A worker for the archive key cannot publish into the resumed view.
    ++m_renderGeneration;
    render(false);
}

namespace {
// Everything setActivity renders from. Polls refresh live fields that Activity
// never shows (goal observation time, cache age, token usage, process load);
// of the processes only their IDs become links. Keep these out of the HTML.
QJsonObject activityInput(QJsonObject details, const QJsonArray &events, const QString &fallbackPrompt, bool tracked)
{
    QJsonArray processes;
    for (const auto &job : details.value("processes").toObject().value("items").toArray()) processes.append(job.toObject().value("id"));
    for (const auto &key : {"goal_observed_at", "cache_hint", "session_usage", "process_work"}) details.remove(QLatin1String(key));
    details["processes"] = processes;
    return {{"details", details}, {"events", events}, {"prompt", fallbackPrompt}, {"tracked", tracked}, {"process_links", ProcessSettings::enabled()}};
}
}

void ActivityView::setActivity(const QJsonObject &details, const QJsonArray &events, const QString &fallbackPrompt, bool tracked)
{
    // Theme, scale, search, cards and local messages render on their own; an
    // identical poll would only regenerate the same journal.
    auto input = activityInput(details, events, fallbackPrompt, tracked);
    if (input == m_activityInput) return;
    m_activityInput = std::move(input);
    const auto conversation = details.value("conversation_id").toString();
    if (!m_conversation.isEmpty() && !conversation.isEmpty() && conversation != m_conversation
        && !details.value("cleared_conversations").toArray().contains(m_conversation)) {
        if(m_voicePlayer)m_voicePlayer->stop();
        m_voiceKey.clear();m_voicePlaying.clear();m_voiceLoading.clear();m_voiceLinks.clear();m_voicePositions.clear();
        m_voicePressed.clear();m_voicePosition=0;m_voicePendingSeek=-1;m_voiceStartAfterLoad=false;
        m_previewFiles.clear(); m_previews.clear(); m_deliveryKeys.clear(); m_attachmentBindings.clear();
        clearMedia();
        m_searchResult = {}; m_searchQuery.clear(); m_browser->setExtraSelections({});
        m_fileLinks.clear(); m_attachmentLinks.clear(); m_messageActions.clear(); m_copyTexts.clear();
        clearDocument();
        m_expanded.clear(); m_knownEvents.clear(); m_initial = true; m_followLatest = true; m_unseen = 0; m_html.clear();
    }
    if (!conversation.isEmpty() && conversation != m_conversation) { ++m_renderGeneration; m_conversation = conversation; }
    m_details = details; m_events = events; m_fallbackPrompt = fallbackPrompt; m_tracked = tracked;
    render(true);
}

void ActivityView::setLocalMessages(const QJsonArray &messages)
{
    if (m_localMessages == messages) return;
    m_localMessages = messages; render(true);
}

void ActivityView::setTimeline(const QJsonObject &details, const QJsonArray &events,
                               const QJsonArray &localMessages, const QString &fallbackPrompt, bool tracked)
{
    // A poll can replace local delivery cards with native events. Commit both
    // inputs together so the document never contains both generations.
    if (m_localMessages != localMessages) { m_localMessages = localMessages; m_activityInput = {}; }
    setActivity(details, events, fallbackPrompt, tracked);
}

void ActivityView::setTaskJournal(const QString &key, const QJsonArray &entries, bool followLatest)
{
    const bool changed = key != m_sessionKey;
    // Task selection keeps its painted document until the prepared replacement
    // is committed. Normal session navigation still clears identities immediately.
    resetSessionKey(key, !key.isEmpty());
    if (changed && !followLatest) { m_initial = false; m_followLatest = false; }
    QJsonArray events;
    for (const auto &value : entries) {
        const auto entry = value.toObject();
        events.append(QJsonObject{{"source", "zerus_task"}, {"type", "TaskEntry"}, {"activity_key", entry.value("id")},
            {"at", entry.value("at").toDouble() / 1000.0}, {"detail", entry.value("text")},
            {"task_author", entry.value("author")}, {"task_label", entry.value("label")}, {"task_role", entry.value("role")},
            {"task_editable", entry.value("editable")}, {"task_delivery", entry.value("delivery")},
            {"attachments", entry.value("attachments")}});
    }
    setActivity({{"task_journal", true}}, events);
}

void ActivityView::setTheme(bool dark)
{
    m_dark = dark;
    auto background = this->palette();
    background.setColor(QPalette::Window, QColor(dark ? canvasDark(m_bubbles) : "#ffffff"));
    setPalette(background); setAutoFillBackground(true);
    QPalette palette = m_browser->palette();
    palette.setColor(QPalette::Base, QColor(dark ? canvasDark(m_bubbles) : "#ffffff"));
    palette.setColor(QPalette::Text, QColor(dark ? "#e8edf4" : "#1a2733"));
    palette.setColor(QPalette::Highlight, QColor(dark ? "#375e54" : "#b6e2d2"));
    palette.setColor(QPalette::HighlightedText, palette.color(QPalette::Text));
    m_browser->setPalette(palette);
    // The workspace's general QTextBrowser style uses a transparent background.
    // That gives Qt a NoBrush Base even after setPalette(), disabling the bounded
    // opaque-canvas chip renderer. Own the same canvas colour here so real
    // workspace scrolls use the fast path, just like a standalone ActivityView.
    const QString browserStyle = QString("QTextBrowser { background:%1; %2 }")
        .arg(dark ? canvasDark(m_bubbles) : "#ffffff", qFuzzyCompare(m_scale, 1.0) ? QString()
            : QString("font-size:%1px;").arg(ContentScale::px(13, m_scale)));
    if (m_browser->styleSheet() != browserStyle) m_browser->setStyleSheet(browserStyle);
    m_latest->setStyleSheet(QString("QPushButton { color:%1; background:%2; border:1px solid %3; border-radius:12px; padding:5px 13px; font-size:11px; } QPushButton:hover { border-color:%1; }"
        " QPushButton[compact=\"true\"] { background:transparent; border:0; border-radius:6px; padding:0; min-width:0; min-height:0; }"
        " QPushButton[compact=\"true\"]:hover { background:%2; }"
        " QPushButton[compact=\"true\"]:focus[keyboardFocus=\"true\"] { border:1px solid %1; }")
        .arg(dark ? "#8bdfc0" : "#167357", dark ? "#233a35" : "#e7f3ed", dark ? "#456e61" : "#a5c8b8"));
    if (m_compactJump) m_latest->setIcon(workspaceIcon("chevrons-down", QColor(dark ? "#8bdfc0" : "#167357")));
    for (auto *edge : m_edges) { static_cast<ColumnEdge *>(edge)->color = QColor(dark ? "#8bdfc0" : "#167357"); edge->update(); }
    m_compaction->setStyleSheet(QString("QWidget#activityCompaction { background:%3; } QLabel { color:%1; background:transparent; font-size:12px; } QProgressBar { background:%2; border:0; border-radius:2px; } QProgressBar::chunk { background:%1; border-radius:2px; }")
        .arg(dark ? "#8bdfc0" : "#167357", dark ? "#34404a" : "#dbe3e9", dark ? canvasDark(m_bubbles) : "#ffffff"));
    m_queue->setStyleSheet(QString("QWidget#activityInputQueue{background:%1;border:1px solid %2;border-radius:7px;} QLabel#queueCaption{color:%3;font-weight:600;}")
        .arg(dark ? "#2d2c23" : "#fff7e4", dark ? "#655638" : "#d7be84", dark ? "#edbd77" : "#805516")
        + (qFuzzyCompare(m_scale, 1.0) ? QString() : QString(" QLabel#queueText{font-size:%1px;}").arg(ContentScale::px(13, m_scale))));
    auto *progress = m_compaction->findChild<QProgressBar *>("compactionProgress");
    auto progressPalette = progress->palette();
    progressPalette.setColor(QPalette::Base, QColor(dark ? "#34404a" : "#dbe3e9"));
    progressPalette.setColor(QPalette::Highlight, QColor(dark ? "#8bdfc0" : "#167357"));
    progress->setPalette(progressPalette);
    render(false);
}

void ActivityView::setContentScale(double scale)
{
    scale = ContentScale::clamp(scale);
    if (qFuzzyCompare(scale, m_scale)) return;
    m_scale = scale;
    // setTheme applies the browser font size together with its opaque canvas;
    // explicit HTML lengths are scaled by the render worker.
    m_queueText->setMaximumHeight(ContentScale::px(100, scale));
    setTheme(m_dark); updateColumnActions();
}

void ActivityView::setColumnWidth(int width)
{
    width = qMax(0, width);
    if (width != m_column) { m_column = width; m_queueColumn->setColumnWidth(width); layoutColumn(); }
    updateColumnActions();
}

namespace {
QString queueSendTooltip(const QString &hint, const QString &chord)
{
    return chord.isEmpty() ? hint : hint.isEmpty() ? chord : hint + " (" + chord + ")";
}
}

void ActivityView::setQueueSendNowShortcut(const QString &chord)
{
    m_queueShortcut = chord;
    m_queueSend->setToolTip(queueSendTooltip(m_details["input_queue"].toObject()["hint"].toString(), chord));
}

int ActivityView::scrollBarReserve() const
{
    // As QAbstractScrollArea lays it out: a shown scroll bar takes its extent,
    // less any overlap, from the side of the viewport.
    auto *bar = m_browser->verticalScrollBar(); auto *container = bar->parentWidget();
    if (!container || container == m_browser || !container->isVisibleTo(m_browser)) return 0;
    return qMax(0, bar->sizeHint().width() - bar->style()->pixelMetric(QStyle::PM_ScrollView_ScrollBarOverlap, nullptr, bar));
}

void ActivityView::layoutColumn()
{
    const int pane = m_browser->width(), reserve = scrollBarReserve(), side = ActivityWidth::inset(pane, m_column);
    // The scroll bar stays at the pane edge and takes its width from the right
    // margin, so showing it does not rewrap the column.
    static_cast<ActivityBrowser *>(m_browser)->setColumnMargins(QMargins(side, 0, qMax(0, side - reserve), 0));
    // Edges need room beside the scroll bar. Without margins the pane edge
    // remains where a text selection starts.
    const auto viewport = m_browser->viewport()->geometry();
    const bool edges = side - reserve >= ColumnEdge::Width;
    m_edges[0]->setGeometry(viewport.left() - ColumnEdge::Width, 0, ColumnEdge::Width, m_browser->height());
    m_edges[1]->setGeometry(viewport.right() + 1, 0, ColumnEdge::Width, m_browser->height());
    for (auto *edge : m_edges) { edge->setVisible(edges); if (edges) edge->raise(); }
}

void ActivityView::dragColumn(int distance)
{
    // Both edges follow the pointer, so the column changes by twice its distance.
    // Its edges stay within the pane and its width keeps a readable minimum.
    const int widest = m_browser->width() - 2 * (scrollBarReserve() + ColumnEdge::Width);
    const int narrowest = qMin(widest, ContentScale::px(ActivityWidth::Minimum, m_scale));
    const int width = qBound(narrowest, m_dragOrigin + 2 * distance, widest);
    if (width != m_column) emit columnWidthRequested(width);
}

void ActivityView::addColumnActions(QMenu *menu)
{
    updateColumnActions();
    if (!menu->isEmpty()) menu->addSeparator();
    menu->addAction(m_fullWidth); menu->addAction(m_resetWidth);
}

void ActivityView::showColumnMenu(const QPoint &position)
{
    auto *menu = new QMenu(this); addColumnActions(menu);
    menu->setAttribute(Qt::WA_DeleteOnClose); menu->popup(position);
}

void ActivityView::updateColumnActions()
{
    m_fullWidth->setChecked(m_column == 0);
    m_resetWidth->setEnabled(m_column != ContentScale::px(ActivityWidth::Default, m_scale));
}

QString ActivityView::plainText() const
{
    QTextCursor all(m_browser->document());
    all.select(QTextCursor::Document);
    return MarkdownObjects::plainText(all);
}

bool ActivityView::replyVisible(const QString &replyId) const
{
    if (renderPending() || !m_searchResult.isEmpty() || !nearBottom() || m_rendering || m_followScheduled) return false;
    const auto anchors = bookmarks(m_browser->document());
    const auto key = "item-" + replyId.section(':', 0, 0);
    int start = anchors.value(key, -1);
    if (start < 0 && !m_details.value("last_message").toString().isEmpty()) start = anchors.value("item-answer-snapshot", -1);
    if (start < 0) {
        // Providers can finish a turn without response text. Its visible Ready
        // state and final operational event are the available result.
        for (const auto &value : m_events) {
            const auto event = value.toObject();
            if (QString::number(event.value("seq").toInteger()) == replyId.section(':', 0, 0)
                && event.value("type") == "Stop" && event.value("detail").toString().isEmpty()
                && m_details.value("activity") == "idle") return true;
        }
        // Catching up to activity after a completed turn acknowledges that
        // turn too, even after its event was pruned from the visible window.
        bool numeric=false;const auto sequence=replyId.section(':',0,0).toLongLong(&numeric);
        if(numeric&&sequence>0&&m_details.value("reply_id").toString()==replyId)
            for(const auto &value:m_events) if(value.toObject().value("seq").toInteger()>=sequence)return true;
        return false;
    }
    QTextCursor cursor(m_browser->document()); cursor.setPosition(start);
    return m_browser->cursorRect(cursor).top() < m_browser->viewport()->height();
}

bool ActivityView::nearBottom() const
{
    auto *bar = m_browser->verticalScrollBar();
    return bar->maximum() - bar->value() <= 30;
}

void ActivityView::scheduleFollow()
{
    if (m_followScheduled) return;
    m_followScheduled = true;
    QTimer::singleShot(0, this, [this] {
        m_followScheduled = false;
        if (m_followLatest && !m_browser->textCursor().hasSelection()) jumpToLatest();
    });
}

void ActivityView::showSearchResult(const QJsonObject &event, const QString &query)
{
    m_searchResult = event; m_searchQuery = query;
    m_initial = false; m_followLatest = false; m_html.clear();
    m_expanded.insert("group-" + eventKey(event), true);
    render(false);
}

void ActivityView::clearSearchResult()
{
    if (m_searchResult.isEmpty()) return;
    m_searchResult = {}; m_searchQuery.clear(); m_browser->setExtraSelections({});
    m_html.clear(); m_initial = true; m_followLatest = true; render(false);
}

void ActivityView::jumpToLatest()
{
    // Hiding a focused button would let Qt focus the next control (context
    // usage). Keep explicit navigation in the journal, without taking focus
    // from the composer during automatic following.
    if (m_latest->hasFocus()) m_browser->setFocus(Qt::OtherFocusReason);
    if (!m_searchResult.isEmpty()) { clearSearchResult(); return; }
    m_rendering = true;
    auto cursor = m_browser->textCursor(); cursor.clearSelection(); m_browser->setTextCursor(cursor);
    m_followLatest = true; m_unseen = 0; m_latest->hide();
    auto *bar = m_browser->verticalScrollBar(); bar->setValue(bar->maximum());
    m_rendering = false;
}

void ActivityView::useCompactJumpButton()
{
    m_compactJump = true; m_latest->setProperty("compact", true);
    m_latest->setFixedSize(24, 24); m_latest->setIconSize(QSize(16, 16));
    setTheme(m_dark); updateJumpButton();
}

void ActivityView::updateJumpButton()
{
    const bool search = !m_searchResult.isEmpty();
    const QString caption = search ? tr("Back to latest activity") : m_unseen > 0
        ? tr("Jump to latest activity (%1 new)").arg(m_unseen) : tr("Jump to latest activity");
    m_latest->setToolTip(caption); m_latest->setAccessibleName(caption);
    m_latest->setText(m_compactJump ? QString() : search ? caption
        : m_unseen > 0 ? tr("↓  %1 new — Jump to latest").arg(m_unseen) : tr("↓  Jump to latest"));
    m_latest->setVisible(search || (!m_followLatest && m_browser->verticalScrollBar()->maximum() > 0));
    positionJumpButton();
}

void ActivityView::positionJumpButton()
{
    if (!m_latest || m_latest->parentWidget()!=m_browser) return;
    const auto viewport = m_browser->viewport()->geometry();
    const auto size = m_latest->sizeHint().boundedTo(QSize(qMax(0,viewport.width()-16),viewport.height()));
    m_latest->resize(size);
    m_latest->move(viewport.left()+(viewport.width()-size.width())/2,
        qMax(viewport.top(),viewport.bottom()+1-size.height()-8));
    m_latest->raise();
}

bool ActivityView::eventFilter(QObject *watched, QEvent *event)
{
    if (watched == m_browser) {
        // Before the scroll area places its viewport for the new width.
        if (event->type() == QEvent::FontChange || event->type() == QEvent::ScreenChangeInternal
#if QT_VERSION >= QT_VERSION_CHECK(6, 6, 0)
            || event->type() == QEvent::DevicePixelRatioChange
#endif
        ) render(false);
        if (event->type() == QEvent::Resize) layoutColumn();
        else if (event->type() == QEvent::Wheel) {
            // The margins scroll the transcript like the column between them.
            if (!m_browser->viewport()->geometry().contains(static_cast<QWheelEvent *>(event)->position().toPoint())) {
                QCoreApplication::sendEvent(m_browser->verticalScrollBar(), event); return true;
            }
        } else if (event->type() == QEvent::ContextMenu) {
            const auto *menu = static_cast<QContextMenuEvent *>(event);
            if (menu->reason() == QContextMenuEvent::Mouse && !m_browser->viewport()->geometry().contains(menu->pos())) {
                showColumnMenu(menu->globalPos()); return true;
            }
        }
    }
    // Copy buttons are images, not links: Tab, Enter and text selection never stop at them.
    if (watched == m_browser->viewport()) {
        auto *viewport = m_browser->viewport();
        const auto *mouse = dynamic_cast<QMouseEvent *>(event);
        if (event->type() == QEvent::ToolTip) {
            const auto *help = static_cast<QHelpEvent *>(event);
            const auto voice=voiceKeyAt(help->pos());
            if(!voice.isEmpty()){
                const auto local=help->pos()-voiceRect(voice).topLeft();
                const auto caption=m_voiceLoading==voice?tr("Loading voice message…")
                    :local.x()>=56*voiceRect(voice).height()/52.?tr("Seek in voice message")
                    :m_voicePlaying==voice?tr("Pause voice message"):tr("Play voice message");
                QToolTip::showText(help->globalPos(),caption,viewport);return true;
            }
            if (!copyKeyAt(help->pos()).isEmpty()) {
                QToolTip::showText(help->globalPos(), tr("Copy as Markdown"), viewport);
                return true;
            }
        } else if (event->type() == QEvent::MouseMove || event->type() == QEvent::Leave) {
            const bool over = mouse && (!copyKeyAt(mouse->position().toPoint()).isEmpty() || !voiceKeyAt(mouse->position().toPoint()).isEmpty()
                                       || !transcriptKeyAt(mouse->position().toPoint()).isEmpty());
            if (over && !m_copyHover) { m_copyCursor = viewport->cursor(); viewport->setCursor(Qt::PointingHandCursor); }
            else if (!over && m_copyHover) viewport->setCursor(m_copyCursor);
            m_copyHover = over;
        } else if (mouse && mouse->button() == Qt::LeftButton
                   && (event->type() == QEvent::MouseButtonPress || event->type() == QEvent::MouseButtonDblClick)) {
            // A press on the button keeps the reader's selection and position.
            m_voicePressed=voiceKeyAt(mouse->position().toPoint());
            if(!m_voicePressed.isEmpty())return true;
            m_transcriptPressed=transcriptKeyAt(mouse->position().toPoint());
            if(!m_transcriptPressed.isEmpty())return true;
            m_copyPressed = copyKeyAt(mouse->position().toPoint());
            if (!m_copyPressed.isEmpty()) return true;
        } else if(mouse && mouse->button()==Qt::LeftButton && event->type()==QEvent::MouseButtonRelease && !m_voicePressed.isEmpty()){
            const auto key=std::exchange(m_voicePressed,QString());
            if(voiceKeyAt(mouse->position().toPoint())==key){
                const auto rect=voiceRect(key);const auto local=mouse->position()-rect.topLeft();
                const qreal scale=rect.height()/52.;
                const auto waveform=VoicePlayerImage::waveformRect(rect.width(),scale);
                if(m_voiceLinks.value(key).value("mime")=="video/mp4")activateVoice(key);
                else if(waveform.contains(local))activateVoice(key,qBound<qreal>(0,(local.x()-waveform.left())/waveform.width(),1));
                else if(local.x()<48*scale)activateVoice(key);
            }
            return true;
        } else if(mouse && mouse->button()==Qt::LeftButton && event->type()==QEvent::MouseButtonRelease && !m_transcriptPressed.isEmpty()){
            const auto key=std::exchange(m_transcriptPressed,QString());
            if(transcriptKeyAt(mouse->position().toPoint())==key)activateLink(QUrl("hgs-activity:"+key));
            return true;
        } else if (mouse && mouse->button() == Qt::LeftButton && event->type() == QEvent::MouseButtonRelease && !m_copyPressed.isEmpty()) {
            const auto pressed = std::exchange(m_copyPressed, QString());
            if (copyKeyAt(mouse->position().toPoint()) == pressed) copyMarkdown(pressed);
            return true;
        }
    }
    if (watched == m_browser->viewport() && (event->type() == QEvent::Resize || event->type() == QEvent::Show)) {
        // A scroll bar that appears or hides moves the viewport's side.
        if (event->type() == QEvent::Resize) layoutColumn();
        if (event->type() == QEvent::Resize && !m_rendering) render(false);
        positionJumpButton();
        if (m_followLatest) scheduleFollow();
    }
    return QWidget::eventFilter(watched, event);
}

void ActivityView::activateLink(const QUrl &url)
{
    if (url == QUrl("hgs-task:edit")) { emit taskDocumentEditRequested(); return; }
    if (url.scheme() == "hgs-activity" && m_toggleKeys.contains(url.path())) {
        const auto key = url.path();
        bool follow = false;
        if (key.startsWith("voice-text-") && m_followLatest && nearBottom() && !m_browser->textCursor().hasSelection()) {
            const int position = m_voicePositions.value(key.mid(QStringLiteral("voice-text-").size()), -1);
            follow = position >= 0;
            // Only the final card follows its new bottom. Opening an older
            // recording must keep the reader's current position instead.
            const auto anchors = bookmarks(m_browser->document());
            for (auto it = anchors.cbegin(); it != anchors.cend(); ++it)
                if (it.value() > position && !it.key().startsWith("item-body-")) { follow = false; break; }
        }
        m_expanded.insert(key, !m_expanded.value(key));
        m_followLatest = follow; render(false); updateJumpButton();
    } else if (url.scheme() == "hgs-file" && m_fileLinks.contains(url.path())) {
        emit fileReferenceActivated(m_fileLinks.value(url.path()));
    } else if (url.scheme() == "hgs-media" && m_mediaLinks.contains(url.path())) {
        emit mediaActivated(url.path(), m_mediaLinks.value(url.path()), m_mediaPaths.value(url.path()));
    } else if(url.scheme()=="hgs-attachment" && m_attachmentLinks.contains(url.path())) {
        emit attachmentActivated(m_attachmentLinks.value(url.path()));
    } else if(url.scheme()=="hgs-voice" && m_voiceLinks.contains(url.path())) {
        activateVoice(url.path());
    } else if(url.scheme()=="hgs-message" && m_messageActions.contains(url.path())) {
        const auto action=m_messageActions.value(url.path());emit messageActionRequested(action.first,action.second);
    } else if(url.scheme()=="hgs-process" && m_processLinks.contains(url.path())) emit processRequested(url.path());
    else if (url.scheme() == "hgs-team-task") emit teamTaskActivated(url.path().toInt());   // WhiteWh fork: agent teams
    else if (webLink(url)) emit externalLinkActivated(url);
}

QString ActivityView::copyButton(const QString &state, const QString &key) const
{
    return QString("hgs-ui:%1/%2/%3?card=%4").arg(state, m_dark ? "dark" : "light", QString::number(qRound(m_scale * 100)), key);
}

QString ActivityView::copyKeyAt(const QPoint &position) const
{
    const QPointF content = position + QPoint(m_browser->horizontalScrollBar()->value(), m_browser->verticalScrollBar()->value());
    const QUrl image(m_browser->document()->documentLayout()->imageAt(content));
    if (image.scheme() != QLatin1String("hgs-ui") || !image.path().startsWith(QLatin1String("copy/"))) return {};
    const auto key = QUrlQuery(image).queryItemValue("card");
    return m_copyTexts.contains(key) ? key : QString();
}

QString ActivityView::voiceKeyAt(const QPoint &position) const
{
    const QPointF content=position+QPoint(m_browser->horizontalScrollBar()->value(),m_browser->verticalScrollBar()->value());
    const QUrl image(m_browser->document()->documentLayout()->imageAt(content));
    return image.scheme()=="hgs-voice-player" && m_voiceLinks.contains(image.path())?image.path():QString();
}

QString ActivityView::transcriptKeyAt(const QPoint &position) const
{
    const QPointF content=position+QPoint(m_browser->horizontalScrollBar()->value(),m_browser->verticalScrollBar()->value());
    const QUrl image(m_browser->document()->documentLayout()->imageAt(content));
    const auto key="voice-text-"+image.path();
    return image.scheme()=="hgs-voice-transcript" && m_toggleKeys.contains(key)?key:QString();
}

QRect ActivityView::voiceRect(const QString &key) const
{
    if(!m_voicePositions.contains(key))return {};
    QTextCursor cursor(m_browser->document());cursor.setPosition(m_voicePositions.value(key));
    const auto at=m_browser->cursorRect(cursor);
    auto *document=paintDocument(m_browser->document());
    const auto image=document->resource(QTextDocument::ImageResource,QUrl("hgs-voice-player:"+key)).value<QImage>();
    const auto size=image.deviceIndependentSize()*document->metrics.logicalDpiY()/document->defaultDpi;
    return QRect(at.topLeft(),size.toSize());
}

void ActivityView::updateVoiceImage(const QString &key)
{
    if(key.isEmpty() || !m_voiceLinks.contains(key) || !m_voicePositions.contains(key) || m_voiceLinks.value(key).value("mime")=="video/mp4")return;
    auto *document=paintDocument(m_browser->document());const QUrl url("hgs-voice-player:"+key);
    const auto previous=document->resource(QTextDocument::ImageResource,url).value<QImage>();
    if(previous.isNull())return;
    document->addResource(QTextDocument::ImageResource,url,VoicePlayerImage::draw(previous.deviceIndependentSize().width(),previous.deviceIndependentSize().height()/52.,
        document->pixelRatio,m_dark,m_voiceLinks.value(key),m_voiceKey==key?m_voicePosition:0,m_voicePlaying==key,
        m_voiceLoading==key || (m_voiceKey==key && m_voicePendingSeek>=0 && m_voiceStartAfterLoad)));
    m_browser->viewport()->update(voiceRect(key).adjusted(-2,-2,2,2));
}

void ActivityView::activateVoice(const QString &key,std::optional<qreal> fraction)
{
    if(!m_voiceLinks.contains(key))return;
    const qint64 duration=m_voiceLinks.value(key).value("duration_ms").toInteger();
    const qint64 position=fraction?qRound64(qBound<qreal>(0,*fraction,1)*duration):0;
    if(m_voiceLoading==key){
        if(fraction){m_voicePosition=position;m_voicePendingSeek=position;updateVoiceImage(key);}
        return;
    }
    if(m_voicePlayer && m_voiceKey==key && !m_voicePlayer->source().isEmpty() && m_voicePlayer->error()==QMediaPlayer::NoError){
        if(fraction){
            m_voicePosition=position;
            if(m_voicePendingSeek>=0)m_voicePendingSeek=position;
            m_voicePlayer->setPosition(position);updateVoiceImage(key);
        }else if(m_voicePlayer->playbackState()==QMediaPlayer::PlayingState || m_voiceStartAfterLoad){
            m_voiceStartAfterLoad=false;m_voicePlayer->pause();
        }else{
            if(m_voicePosition>=duration){m_voicePosition=0;m_voicePlayer->setPosition(0);if(m_voicePendingSeek>=0)m_voicePendingSeek=0;}
            if(m_voicePendingSeek>=0)m_voiceStartAfterLoad=true;else m_voicePlayer->play();
        }
        updateVoiceImage(key);return;
    }
    if(m_videoDialog)m_videoDialog->close();
    const auto previous=m_voiceKey;
    if(m_voicePlayer){m_voicePlayer->stop();m_voicePlayer->setSource({});}
    m_voiceKey=key;m_voicePosition=position;m_voicePendingSeek=position;m_voiceStartAfterLoad=!fraction.has_value();
    const bool hadError=!m_voiceError.isEmpty();
    m_voiceLoading=key;m_voiceError.clear();m_voicePlaying.clear();
    updateVoiceImage(previous);updateVoiceImage(key);
    if(hadError)render(false);
    emit voicePlaybackRequested(key,m_voiceLinks.value(key));
}

void ActivityView::copyMarkdown(const QString &key)
{
    QGuiApplication::clipboard()->setText(m_copyTexts.value(key));
    // Only the pressed button's image changes. Its size stays, so the document
    // needs no edit and the reading position and selection stay where they are.
    auto *document = paintDocument(m_browser->document());
    const QUrl button(copyButton("copy", key));
    document->addResource(QTextDocument::ImageResource, button, cardButton(QUrl(copyButton("copied", key)), document->pixelRatio));
    m_browser->viewport()->update();
    QToolTip::showText(QCursor::pos(), tr("Copied as Markdown"), m_browser->viewport());
    QTimer::singleShot(1500, this, [this, button] {
        auto *document = paintDocument(m_browser->document());
        document->addResource(QTextDocument::ImageResource, button, cardButton(button, document->pixelRatio));
        m_browser->viewport()->update();
    });
}

void ActivityView::clearMedia()
{
    qDeleteAll(m_movies); m_movies.clear();
    m_mediaTiles.clear(); m_mediaLinks.clear(); m_mediaPaths.clear(); m_mediaRequested.clear();
}

void ActivityView::setMediaPreview(const QString &key, const QString &path, const QString &error)
{
    if (!m_mediaLinks.contains(key)) return;
    const auto href = m_mediaLinks.value(key);
    const auto file = SessionFileReference::parse(href).path;
    const auto name = QFileInfo(file).fileName();
    const auto kind = MediaFiles::kind(file);
    const auto install = [this, key](const QImage &tile) {
        m_mediaTiles.insert(key, tile);
        paintDocument(m_browser->document())->addResource(QTextDocument::ImageResource, QUrl("hgs-media:" + key), tile);
        // A preview keeps its reserved size: JournalImages draws the current
        // resource, so a repaint shows it without a relayout.
        m_browser->viewport()->update();
    };
    if (!path.isEmpty()) m_mediaPaths.insert(key, path);
    if (path.isEmpty() || !error.isEmpty()) {
        install(ActivityMedia::tile({}, name, kind == MediaFiles::Kind::Video, error.isEmpty() ? tr("Click to open") : error));
        return;
    }
    const QSize bound(ActivityMedia::Width * 2, ActivityMedia::Height * 2);
    // #744: the file (in a session's folder) is read by PathProbe's worker,
    // bounded in time, never on this thread.
    if (kind == MediaFiles::Kind::Animation) {
        PathProbe::instance().read(path, MediaFiles::PreviewLimit, this, [this, key, name, install](bool ok, const QByteArray &bytes) {
            if (!ok) { install(ActivityMedia::tile({}, name, false, tr("This picture cannot be shown"))); return; }
            delete m_movies.take(key);
            auto *buffer = new QBuffer;
            buffer->setData(bytes);
            auto *movie = new QMovie(buffer, {}, this);
            buffer->setParent(movie);
            m_movies.insert(key, movie);
            connect(movie, &QMovie::frameChanged, this, [install, movie, name] { install(ActivityMedia::tile(movie->currentImage(), name, false, {})); });
            if (isVisible()) movie->start(); else movie->jumpToFrame(0);
        });
        return;
    }
    if (kind == MediaFiles::Kind::Video) {
        install(ActivityMedia::tile({}, name, true, {}));
        MediaFiles::poster(path, this, [install, name](const QImage &frame) { install(ActivityMedia::tile(frame, name, true, {})); });
        return;
    }
    PathProbe::instance().run(this, [path, bound] { return QVariant::fromValue(MediaFiles::picture(path, bound)); },
                              [install, name](bool ok, const QVariant &value) {
        const auto image = ok ? value.value<QImage>() : QImage();
        install(ActivityMedia::tile(image, name, false, image.isNull() ? tr("This picture cannot be shown") : QString()));
    });
}

void ActivityView::setAttachmentPreview(const QString &key, const QImage &image)
{
    if (image.isNull() || !m_previewFiles.contains(key) || !m_attachmentLinks.contains(key)) return;
    QImage tile(192, 128, QImage::Format_ARGB32_Premultiplied); tile.fill(Qt::transparent);
    const auto scaled = image.scaled(tile.size(), Qt::KeepAspectRatio, Qt::SmoothTransformation);
    QPainter painter(&tile); painter.drawImage((tile.width()-scaled.width())/2, (tile.height()-scaled.height())/2, scaled); painter.end();
    m_previews.insert(key, tile);
    paintDocument(m_browser->document())->addResource(QTextDocument::ImageResource, QUrl("hgs-thumbnail:"+key), tile);
    // The reserved image dimensions stay constant. No setHtml, scroll reset or
    // selection change is needed when a remote thumbnail arrives.
    m_browser->document()->markContentsDirty(0, m_browser->document()->characterCount());
    m_browser->viewport()->update();
}

void ActivityView::render(bool contentUpdate)
{
    if (contentUpdate) ++m_contentRevision;
    else ++m_renderGeneration;
    m_renderDirty = true;
    m_pendingContentUpdate |= contentUpdate;
    const bool searching = !m_searchResult.isEmpty();
    const QJsonObject details = searching ? QJsonObject() : m_details;
    const auto queue = details["input_queue"].toObject();
    m_queue->setVisible(m_tracked && !searching && !queue["id"].toString().isEmpty());
    if (m_queueText->text() != queue["text"].toString()) m_queueText->setText(queue["text"].toString());
    m_queueText->setToolTip(queue["text"].toString());
    m_queueSend->setEnabled(queue["can_send_now"].toBool() && !details["queue_sending"].toBool());
    m_queueSend->setText(details["queue_sending"].toBool() ? tr("Sending…") : tr("Send now"));
    m_queueSend->setToolTip(queueSendTooltip(queue["hint"].toString(), m_queueShortcut));
    // Live state is separate from journal HTML: its animation and polls must
    // not rebuild the transcript or disturb a text selection.
    m_compaction->setVisible(m_tracked && !searching && details.value("phase") == "compacting"
        && details.value("runtime_state") == "live" && details.value("process_state") == "running"
        && details.value("state") != "archived");
    if (!m_renderScheduled) {
        m_renderScheduled = true;
        QTimer::singleShot(0, this, [this] { m_renderScheduled = false; startRender(); });
    }
}

void ActivityView::startRender()
{
    if (renderClosingApp == qApp || !isVisible() || m_renderInFlight || m_renderWorkers >= 2 || !m_renderDirty) return;
    m_renderDirty = false;
    m_renderInFlight = true;
    ++m_renderWorkers;
    ++m_renderPasses;
    auto job = std::make_shared<RenderJob>();
    auto result = job->result;
    result->generation = m_renderGeneration;
    result->contentRevision = m_contentRevision;
    result->m_sessionKey = m_sessionKey; result->m_conversation = m_conversation;
    result->m_details = m_details; result->m_searchResult = m_searchResult;
    result->m_events = m_events; result->m_localMessages = m_localMessages;
    result->m_fallbackPrompt = m_fallbackPrompt;
    result->m_expanded = m_expanded; result->m_deliveryKeys = m_deliveryKeys; result->m_attachmentBindings = m_attachmentBindings;
    result->m_previews = m_previews;
    result->m_mediaTiles = m_mediaTiles;
    result->m_voiceError=m_voiceError;
    result->m_dark = m_dark; result->m_tracked = m_tracked; result->m_scale = m_scale; result->m_bubbles = m_bubbles;
    result->processesEnabled = ProcessSettings::enabled();
    result->width = m_browser->viewport()->width();
    result->viewportHeight = m_browser->viewport()->height();
    const int side = ActivityWidth::inset(m_browser->width(), m_column);
    auto *bar = m_browser->verticalScrollBar();
    const int reserve = qMax(0, bar->sizeHint().width() - bar->style()->pixelMetric(QStyle::PM_ScrollView_ScrollBarOverlap, nullptr, bar));
    result->widthWithScrollBar = qMax(1, m_browser->width() - reserve - side - qMax(0, side - reserve));
    const QFont font = m_browser->font();
    const auto option = m_browser->document()->defaultTextOption();
    const qreal ratio = m_browser->devicePixelRatioF();
    const int dpiX = m_browser->logicalDpiX(), dpiY = m_browser->logicalDpiY();
    QThread *guiThread = thread();
    auto *lifetime = renderLifetime();
    QPointer<ActivityView> view(this);
    auto *worker = QThread::create([result, job, view, lifetime, font, option, ratio, dpiX, dpiY, guiThread] {
        result->document = std::make_unique<JournalDocument>();
        auto *document = result->document.get();
        document->pixelRatio = ratio;
        document->metrics = QImage(1, 1, QImage::Format_ARGB32_Premultiplied);
        document->defaultDpi = document->metrics.logicalDpiY();
        document->metrics.setDotsPerMeterX(qRound(dpiX / .0254));
        document->metrics.setDotsPerMeterY(qRound(dpiY / .0254));
        document->documentLayout()->setPaintDevice(&document->metrics);
        document->setDefaultFont(font);
        document->setDefaultTextOption(option);
        result->build();
        MarkdownObjects::prepareChips(document);
        // Copy the text model without a second HTML import or layout.
        auto target = std::make_unique<JournalDocument>();
        target->setLayoutEnabled(false);
        target->setUndoRedoEnabled(false);
        target->pixelRatio = ratio;
        target->setDefaultFont(font);
        target->setDefaultTextOption(option);
        QTextCursor(target.get()).insertFragment(QTextDocumentFragment(document));
        target->rootFrame()->setFrameFormat(document->rootFrame()->frameFormat());
        target->setPageSize(document->pageSize());
        target->layoutSource = document;
        target->setDocumentLayout(new JournalLayout(target.get(), document));
        target->setLayoutEnabled(true);
        Q_ASSERT(target->characterCount() == document->characterCount());
        // QWidgetTextControl reads block.layout() directly for cursor geometry,
        // keyboard movement and selection. Reproduce those lines on the worker;
        // delegating blockBoundingRect alone would leave the clone unselectable.
        for (auto block = target->begin(); block.isValid(); block = block.next()) {
            const auto sourceBlock = document->findBlock(block.position());
            const auto *sourceLayout = sourceBlock.layout();
            auto *text = block.layout();
            text->setFont(sourceLayout->font());
            text->setTextOption(sourceLayout->textOption());
            text->setFormats(sourceLayout->formats());
            text->setPosition(sourceLayout->position());
            text->beginLayout();
            for (int i = 0; i < sourceLayout->lineCount(); ++i) {
                const auto from = sourceLayout->lineAt(i);
                auto line = text->createLine();
                if (!line.isValid()) break;
                line.setLeadingIncluded(from.leadingIncluded());
                line.setLineWidth(from.width());
                line.setPosition(from.position());
            }
            text->endLayout();
        }
        result->document.release();
        result->document = std::move(target);
        document = result->document.get();
        // Finish the layout's pending size notification on its owning worker.
        // At shutdown there may be no GUI event turn to register a moved timer
        // before its document is deleted. This dispatcher owns only this job.
        QCoreApplication::processEvents(QEventLoop::AllEvents);
        // Transfer from its owning thread, including layout/inline-code children.
        document->moveToThread(guiThread);
        job->document = document;
        auto *worker = QThread::currentThread();
        // QObject emits destroyed before destroying its children. Queue the
        // release so source/layout/font destruction finishes first. Keeping
        // this dedicated thread idle preserves its platform font faces without
        // sharing mutable glyph caches with another rendering job.
        QObject::connect(document, &QObject::destroyed, lifetime, [worker] { worker->quit(); }, Qt::QueuedConnection);
        job->prepared.release();
        QMetaObject::invokeMethod(lifetime, [view, result, job] {
            if (view) {
                view->m_renderInFlight = false;
                if (view->isVisible() && result->generation == view->m_renderGeneration) {
                    view->commitRender(result);
                    job->browser = view->m_browser;
                } else view->m_renderDirty = true;
            }
            // A stale, hidden or identical-HTML result was not installed. Drop
            // its document now, otherwise the parked job would retain itself.
            result->document.reset();
            if (view) view->startRender();
        }, Qt::QueuedConnection);
        // QThread::quit also exits this loop, including a release that arrives
        // before exec: Qt's quitNow flag makes exec return immediately then.
        QEventLoop loop;
        loop.exec();
    });
    // The application owns cleanup even if navigation destroys this view while
    // the bounded one-job worker is finishing. Destruction never waits for it.
    worker->setParent(lifetime);
    worker->setObjectName("activityRenderWorker");
    lifetime->jobs.insert(worker, job);
    connect(worker, &QThread::finished, lifetime, [view, worker, lifetime] {
        lifetime->jobs.remove(worker);
        worker->deleteLater();
        if (view) { --view->m_renderWorkers; view->startRender(); }
    });
    // Not below the GUI's priority (#523): a below-normal worker gets almost no
    // CPU while normal-priority work - the agents' builds and tests - fills the
    // cores, and Activity then never renders. The GUI stays responsive because
    // the preparation runs on its own thread, not because it waits.
    worker->start(QThread::NormalPriority);
}

void ActivityView::commitRender(const std::shared_ptr<ActivityRenderResult> &result)
{
    const bool searching = !m_searchResult.isEmpty();
    const auto *previous = paintDocument(m_browser->document());
    const auto *next = result->document->layoutSource;
    if (result->m_html == m_html && previous->textWidth() == result->width
        && previous->defaultFont() == next->defaultFont()
        && previous->pixelRatio == next->pixelRatio
        && previous->metrics.dotsPerMeterX() == next->metrics.dotsPerMeterX()
        && previous->metrics.dotsPerMeterY() == next->metrics.dotsPerMeterY()) {
        // Attachment location metadata can change without changing its identity
        // or markup. Keep the committed action maps current even in that case.
        m_attachmentLinks = result->m_attachmentLinks;m_voiceLinks=result->m_voiceLinks;m_voicePositions=result->m_voicePositions;
        for(const auto &key:m_voiceLinks.keys())updateVoiceImage(key);
        m_messageActions = result->m_messageActions;
        m_pendingContentUpdate = result->contentRevision != m_contentRevision;
        return;
    }
    const bool hasContent = result->content;
    const bool follow = !searching && ((m_initial && hasContent) || (m_followLatest && !m_browser->textCursor().hasSelection()));
    if (m_pendingContentUpdate && !m_initial && !follow) {
        QSet<QString> added = result->m_knownEvents; added.subtract(m_knownEvents);
        m_unseen += qMax(added.size(), qsizetype(added.isEmpty() && result->m_knownEvents == m_knownEvents ? 1 : 0));
    }
    m_pendingContentUpdate = result->contentRevision != m_contentRevision;
    m_knownEvents = result->m_knownEvents; m_html = result->m_html;
    m_fileLinks = result->m_fileLinks; m_attachmentLinks = result->m_attachmentLinks;m_voiceLinks=result->m_voiceLinks;m_voicePositions=result->m_voicePositions;
    m_messageActions = result->m_messageActions; m_copyTexts = result->m_copyTexts;
    m_expanded = result->m_expanded; m_deliveryKeys = result->m_deliveryKeys; m_attachmentBindings = result->m_attachmentBindings;
    m_toggleKeys = result->m_toggleKeys; m_processLinks = result->m_processLinks;
    QStringList requestPreviews;
    for (auto it = result->m_previewFiles.cbegin(); it != result->m_previewFiles.cend(); ++it)
        if (!m_previewFiles.contains(it.key())) requestPreviews.append(it.key());
    m_previewFiles = result->m_previewFiles;
    QStringList requestMedia;
    for (auto it = result->m_mediaLinks.cbegin(); it != result->m_mediaLinks.cend(); ++it)
        if (!m_mediaRequested.contains(it.key())) { m_mediaRequested.insert(it.key()); requestMedia.append(it.key()); }
    m_mediaLinks = result->m_mediaLinks;
    if (hasContent) m_initial = false;
    const auto anchors = bookmarks(m_browser->document());
    const auto cursor = m_browser->textCursor();
    const Position selectionStart(cursor.anchor(), anchors), selectionEnd(cursor.position(), anchors);
    const auto top = m_browser->cursorForPosition(QPoint(2, 2));
    const Position topPosition(top.position(), anchors);
    const int topY = m_browser->cursorRect(top).top();
    m_rendering = true;
    // A document replacement emits intermediate geometry/scroll updates.
    // Keep this transaction synchronous: returning to the event loop with
    // updates disabled lets a parent repaint erase Activity for a whole frame.
    const bool updatesEnabled = m_browser->updatesEnabled();
    m_browser->setUpdatesEnabled(false);
    auto *oldDocument = m_browser->document();
    auto *document = result->document.release();
    // Thumbnails may have arrived while the snapshot was building. Their fixed
    // dimensions permit the current resource to be installed without relayout.
    for (auto it = m_previews.cbegin(); it != m_previews.cend(); ++it)
        paintDocument(document)->addResource(QTextDocument::ImageResource, QUrl("hgs-thumbnail:" + it.key()), it.value());
    for (auto it = m_mediaTiles.cbegin(); it != m_mediaTiles.cend(); ++it)
        paintDocument(document)->addResource(QTextDocument::ImageResource, QUrl("hgs-media:" + it.key()), it.value());
    document->setParent(m_browser);
    m_browser->setDocument(document);
    for(const auto &key:m_voiceLinks.keys())updateVoiceImage(key);
    oldDocument->deleteLater();
    emit document->documentLayout()->documentSizeChanged(result->size);
    const auto &updatedAnchors = result->anchors;
    const int length = m_browser->document()->characterCount();
    QTextCursor restored(m_browser->document()); restored.setPosition(selectionStart.restored(updatedAnchors, length));
    restored.setPosition(selectionEnd.restored(updatedAnchors, length), QTextCursor::KeepAnchor);
    if (m_taskJournalReset) restored.setPosition(0);
    m_browser->setTextCursor(restored);
    if (m_taskJournalReset) {
        if (!follow) m_browser->verticalScrollBar()->setValue(0);
        m_taskJournalReset = false;
    } else if (!follow) {
        QTextCursor restoredTop(m_browser->document()); restoredTop.setPosition(topPosition.restored(updatedAnchors, length));
        auto *bar = m_browser->verticalScrollBar();
        bar->setValue(bar->value() + m_browser->cursorRect(restoredTop).top() - topY);
    }
    m_rendering = false; m_followLatest = follow;
    if (searching) {
        QList<QTextEdit::ExtraSelection> highlights; QTextCursor found(m_browser->document()), first;
        while (highlights.size() < 300 && !m_searchQuery.isEmpty()) {
            found = m_browser->document()->find(m_searchQuery, found);
            if (found.isNull()) break;
            if (first.isNull()) first = found;
            QTextEdit::ExtraSelection h; h.cursor = found; h.format.setBackground(QColor(m_dark ? "#605032" : "#ffebb4"));
            h.format.setForeground(QColor(m_dark ? "#ffdda0" : "#6e4b00")); highlights.append(h);
        }
        m_browser->setExtraSelections(highlights);
        if (!first.isNull()) { first.clearSelection(); m_browser->setTextCursor(first); m_browser->ensureCursorVisible(); }
        updateJumpButton();
    } else if (follow) { jumpToLatest(); scheduleFollow(); }
    else updateJumpButton();
    m_browser->setUpdatesEnabled(updatesEnabled);
    for (const auto &key : requestMedia) {
        const auto session = m_sessionKey;
        QTimer::singleShot(0, this, [this, key, session] {
            if (session == m_sessionKey && m_mediaLinks.contains(key)) emit mediaPreviewRequested(key, m_mediaLinks.value(key));
        });
    }
    for (const auto &key : requestPreviews) {
        const auto session = m_sessionKey, conversation = m_conversation;
        QTimer::singleShot(0, this, [this, key, session, conversation] {
            if (session == m_sessionKey && conversation == m_conversation && m_attachmentLinks.contains(key))
                emit attachmentPreviewRequested(key, m_attachmentLinks.value(key));
        });
    }
}

void ActivityView::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    for (auto *movie : std::as_const(m_movies)) movie->state() == QMovie::Paused ? movie->setPaused(false) : movie->start();
    if (m_renderDirty) render(false);
}

void ActivityView::clearDocument()
{
    auto *old = m_browser->document();
    auto *empty = new JournalDocument(m_browser);
    empty->setDefaultFont(m_browser->font());
    empty->setDefaultTextOption(old->defaultTextOption());
    m_browser->setDocument(empty);
    old->deleteLater();
}

#include "ActivityView.moc"

void ActivityView::playVoice(const QString &key,const QString &path,const QString &error)
{
    if(m_voiceLoading!=key || !m_voiceLinks.contains(key) || !isVisible())return;
    m_voiceLoading.clear();
    if(!error.isEmpty() || path.isEmpty()){m_voicePendingSeek=-1;m_voiceStartAfterLoad=false;m_voiceError=key+"\n"+(error.isEmpty()?tr("Recording is unavailable."):error);render(false);return;}
    if(!m_voicePlayer){
        m_voicePlayer=new QMediaPlayer(this);registerMessagePlayer(m_voicePlayer);m_voiceOutput=new QAudioOutput(this);m_voicePlayer->setAudioOutput(m_voiceOutput);
        connect(m_voicePlayer,&QMediaPlayer::playbackStateChanged,this,[this](QMediaPlayer::PlaybackState state){m_voicePlaying=state==QMediaPlayer::PlayingState?m_voiceKey:QString();updateVoiceImage(m_voiceKey);});
        connect(m_voicePlayer,&QMediaPlayer::positionChanged,this,[this](qint64 position){m_voicePosition=position;updateVoiceImage(m_voiceKey);});
        connect(m_voicePlayer,&QMediaPlayer::mediaStatusChanged,this,[this](QMediaPlayer::MediaStatus status){
            if((status!=QMediaPlayer::LoadedMedia && status!=QMediaPlayer::BufferedMedia) || m_voicePendingSeek<0 || !isVisible())return;
            const auto seek=std::exchange(m_voicePendingSeek,qint64(-1));m_voicePlayer->setPosition(seek);
            if(std::exchange(m_voiceStartAfterLoad,false))m_voicePlayer->play();
        });
        connect(m_voicePlayer,&QMediaPlayer::errorOccurred,this,[this](QMediaPlayer::Error,const QString &detail){m_voiceError=m_voiceKey+"\n"+tr("Could not play recording: %1").arg(detail);render(false);});
    }
    if(m_voiceLinks.value(key).value("mime")=="video/mp4"){
        if(m_videoDialog)m_videoDialog->close();
        auto *dialog=new QDialog(this);dialog->setAttribute(Qt::WA_DeleteOnClose);dialog->setWindowTitle(tr("Video message"));m_videoDialog=dialog;
        auto *layout=new QVBoxLayout(dialog);auto *video=new RoundVideoView(dialog);layout->addWidget(video,0,Qt::AlignHCenter);
        auto *toggle=new QPushButton(tr("Play / pause"),dialog);layout->addWidget(toggle);
        connect(toggle,&QPushButton::clicked,dialog,[this]{if(m_voicePlayer->playbackState()==QMediaPlayer::PlayingState)m_voicePlayer->pause();else m_voicePlayer->play();});
        connect(dialog,&QDialog::finished,this,[this,dialog]{if(m_videoDialog==dialog){m_voicePlayer->stop();m_voicePlayer->setVideoSink(nullptr);m_videoDialog=nullptr;}});
        m_voicePlayer->setVideoSink(video->sink());dialog->show();
    } else m_voicePlayer->setVideoSink(nullptr);
    m_voiceKey=key;m_voicePlayer->setSource(QUrl::fromLocalFile(path));
    updateVoiceImage(key);
}
void ActivityView::hideEvent(QHideEvent *event)
{
    for (auto *movie : std::as_const(m_movies)) movie->setPaused(true);
    if(m_videoDialog)m_videoDialog->close();
    m_voicePendingSeek=-1;m_voiceStartAfterLoad=false;m_voicePressed.clear();
    if(m_voicePlayer)m_voicePlayer->stop();m_voiceLoading.clear();updateVoiceImage(m_voiceKey);
    QWidget::hideEvent(event);
}
