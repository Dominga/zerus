#pragma once
#include <QFrame>
#include <QLabel>
#include <QMap>
#include <QPainter>
#include <QPainterPath>
#include <QScrollArea>
#include <QScrollBar>
#include <QTimer>
#include <QVBoxLayout>
#include <QVector>
#include <functional>

#include "CollapsibleBlock.h"
#include "WorkspaceSkin.h"

// WhiteWh fork, U25 part 6 (spec 6): the session inspector as one column of
// collapsible cards instead of tabs - Environment, Context, Team, Tasks, Iron
// rules, Worktrees, Changes, Project tasks (upstream's durable Zerus tasks,
// its Tasks tab) and the optional SocratiCode extension. Each
// card has radius 12 on the skin's card colour, 8 px between cards and no
// divider on the panel's left. A card folds by its "Title ▾" header and keeps
// its state under "ui/collapsed/inspector/<key>"; a short value sits on the
// right of the header. open() unfolds a card and scrolls to it, for the links
// from outside (Open team panel, N iron rules, the git chips).
class InspectorCards : public QScrollArea {
public:
    enum Card { Environment, Context, Team, Tasks, IronRules, Worktrees, Changes, SocratiCode, ProjectTasks };

    explicit InspectorCards(QWidget *parent = nullptr) : QScrollArea(parent)
    {
        setObjectName("sessionInspector");
        setWidgetResizable(true);
        setFrameShape(QFrame::NoFrame);
        setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        auto *column = new QWidget;
        column->setObjectName("inspectorColumn");
        m_column = new QVBoxLayout(column);
        m_column->setContentsMargins(10, 10, 10, 16);
        m_column->setSpacing(8);
        // The spec's open/collapsed defaults.
        add(Environment, "environment", QStringLiteral("Environment"), true);
        add(Context, "context", QStringLiteral("Context"), true);
        add(Team, "team", QStringLiteral("Team"), true);
        add(Tasks, "tasks", QStringLiteral("Tasks"), true);
        add(IronRules, "rules", QStringLiteral("Iron rules"), false);
        add(Worktrees, "worktrees", QStringLiteral("Worktrees"), false);
        add(Changes, "changes", QStringLiteral("Changes"), false);
        add(ProjectTasks, "projecttasks", QStringLiteral("Project tasks"), false);
        add(SocratiCode, "socraticode", QStringLiteral("SocratiCode"), true);
        m_column->addStretch(1);
        setWidget(column);
        setDark(true);
    }

    static QString key(Card card)
    {
        static const char *keys[] = {"environment", "context", "team", "tasks", "rules", "worktrees", "changes", "socraticode", "projecttasks"};
        return QString::fromLatin1(keys[card]);
    }

    QFrame *card(Card card) const { return m_cards.value(card).frame; }
    CollapsibleBlock *block(Card card) const { return m_cards.value(card).block; }
    QVBoxLayout *body(Card card) const { return m_cards.value(card).block->body(); }
    // Right of the header, in both states: a badge or button.
    QHBoxLayout *trailing(Card card) const { return m_cards.value(card).block->trailing(); }

    // The short value right of the title ("2 of 5 completed", "36%"). With
    // `collapsedOnly` it shows only while the card is folded.
    void setValue(Card card, const QString &text, bool collapsedOnly = false)
    {
        auto &entry = m_cards[card];
        entry.value->setText(text);
        entry.valueCollapsedOnly = collapsedOnly;
        refreshValue(card);
    }
    QString value(Card card) const { return m_cards.value(card).value->text(); }

    // Unfolds the card and brings it into view.
    void open(Card card)
    {
        auto entry = m_cards.value(card);
        if (!entry.frame) return;
        entry.frame->show();
        entry.block->setExpanded(true);
        QTimer::singleShot(0, this, [this, frame = entry.frame] {
            widget()->layout()->activate();
            verticalScrollBar()->setValue(frame->y() - 4);
        });
        if (opened) opened(card);
    }
    std::function<void(Card)> opened;

protected:
    // The column is as wide as the panel: a wide card clips its content
    // instead of pushing the header buttons out of view.
    void resizeEvent(QResizeEvent *event) override
    {
        QScrollArea::resizeEvent(event);
        if (widget() && widget()->width() != viewport()->width()) widget()->setFixedWidth(viewport()->width());
    }

public:

    void setDark(bool dark)
    {
        const auto &skin = WorkspaceSkin::active();
        const auto t = [&skin, dark](const char *name) { return WorkspaceSkin::color(skin, dark, QString::fromLatin1(name)).name(); };
        // .ibody2 / .icard / .ihd of the prototype. The column has no backdrop
        // of its own: the cards sit on the session panel's surface.
        setStyleSheet(QStringLiteral(
            "QScrollArea#sessionInspector,QScrollArea#sessionInspector>QWidget#qt_scrollarea_viewport,"
            "QWidget#inspectorColumn{background:transparent;border:0;}"
            "QFrame#inspectorCard{background:%1;border:1px solid %2;border-radius:12px;}"
            "QFrame#inspectorCard QLabel{background:transparent;}"
            "QLabel#inspectorCardValue{color:%3;font-size:12px;}"
            "QFrame#inspectorCard QLabel#collapsibleTitle{font-size:12.5px;font-weight:600;}"
            // .irow: the Environment rows, the key row's mono ID, dim keys.
            "QFrame#inspectorCard QLabel#inspectorEnvValue,QFrame#inspectorCard QLabel#keepCacheWarmLabel{color:%4;font-size:12px;}"
            "QFrame#inspectorCard QLabel#inspectorEnvKey,QFrame#inspectorCard QLabel#keepCacheWarmLimit{color:%3;font-size:12px;}"
            "QFrame#inspectorCard QLabel#detailsIdentityValue{color:%4;font-family:monospace;font-size:11.5px;}"
            "QFrame#inspectorCard QLabel#inspectorContextUsed{color:%5;font-size:12.5px;font-weight:600;}"
            "QFrame#inspectorCard QLabel#inspectorContextTokens{color:%3;font-family:monospace;font-size:12px;}"
            "QFrame#inspectorCard QLabel#inspectorContextLeft,QFrame#inspectorCard QLabel#inspectorChangesEmpty{color:%3;font-size:11px;}"
            "QFrame#inspectorCard QLabel#inspectorChangePath{color:%4;font-family:monospace;font-size:11.5px;}"
            "QFrame#inspectorCard QTextBrowser{background:transparent;border:0;}"
            // A panel inside a card (Team) draws on the card, not the window.
            "QFrame#inspectorCard QScrollArea,QFrame#inspectorCard QScrollArea>QWidget>QWidget{background:transparent;border:0;}"
            // The prototype's switch: a pill, the accent when on.
            "QCheckBox#keepCacheWarm::indicator{width:30px;height:18px;border-radius:9px;background:%6;border:1px solid %2;}"
            "QCheckBox#keepCacheWarm::indicator:checked{background:%7;border-color:%7;}")
            .arg(t("card"), t("cardBorder"), t("dim"), t("text2"), t("fg"), t("border"), t("accent")));
        for (auto &entry : m_cards) entry.block->setDark(dark);
    }

private:
    struct Entry {
        QFrame *frame = nullptr;
        CollapsibleBlock *block = nullptr;
        QLabel *value = nullptr;
        bool valueCollapsedOnly = false;
    };

    void add(Card card, const char *key, const QString &title, bool open)
    {
        auto *frame = new QFrame;
        frame->setObjectName("inspectorCard");
        frame->setProperty("card", QString::fromLatin1(key));
        frame->setAttribute(Qt::WA_StyledBackground);
        auto *v = new QVBoxLayout(frame);
        v->setContentsMargins(8, 6, 12, 12);
        v->setSpacing(0);
        auto *block = new CollapsibleBlock(QStringLiteral("inspector/") + QString::fromLatin1(key), title, open);
        block->setProperty("card", QString::fromLatin1(key));
        block->body()->setContentsMargins(4, 4, 0, 0);
        block->body()->setSpacing(8);
        auto *value = new QLabel;
        value->setObjectName("inspectorCardValue");
        value->hide();
        block->trailing()->addWidget(value);
        v->addWidget(block);
        m_column->addWidget(frame);
        m_cards.insert(card, Entry{frame, block, value, false});
        block->expandedChanged = [this, card](bool) {
            refreshValue(card);
            // A folded card keeps only its header: no trailing margin under it.
            auto entry = m_cards.value(card);
            entry.frame->layout()->setContentsMargins(8, 6, 12, entry.block->isExpanded() ? 12 : 6);
            if (toggled) toggled(card, entry.block->isExpanded());
        };
        frame->layout()->setContentsMargins(8, 6, 12, block->isExpanded() ? 12 : 6);
    }

    void refreshValue(Card card)
    {
        const auto entry = m_cards.value(card);
        entry.value->setVisible(!entry.value->text().isEmpty() && (!entry.valueCollapsedOnly || !entry.block->isExpanded()));
    }

public:
    std::function<void(Card, bool)> toggled;

private:
    QVBoxLayout *m_column = nullptr;
    QMap<Card, Entry> m_cards;
};

// A one-line value that elides instead of widening the column; text() and
// the tooltip keep the whole value.
class InspectorValue : public QLabel {
public:
    explicit InspectorValue(Qt::TextElideMode mode = Qt::ElideRight, QWidget *parent = nullptr) : QLabel(parent), m_mode(mode)
    {
        setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        setMinimumWidth(0);
    }
    void setValue(const QString &text)
    {
        if (this->text() == text) return;
        setText(text);
        setToolTip(text);
        update();
    }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.setPen(palette().color(foregroundRole()));
        p.setFont(font());
        const QRect area = contentsRect();
        p.drawText(area, Qt::AlignLeft | Qt::AlignVCenter, fontMetrics().elidedText(text(), m_mode, area.width()));
    }

private:
    Qt::TextElideMode m_mode;
};

// U25 part 6 (spec 6): the Context card's composite bar - one segment per
// reported part, the rest of the window in the border colour.
class ContextBar : public QWidget {
public:
    explicit ContextBar(QWidget *parent = nullptr) : QWidget(parent) { setObjectName("inspectorContextBar"); setFixedHeight(4); }
    struct Part { double fraction; QColor color; };
    void setParts(const QVector<Part> &parts, const QColor &rest) { m_parts = parts; m_rest = rest; update(); }
    QVector<Part> parts() const { return m_parts; }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        QPainterPath clip;
        clip.addRoundedRect(rect(), 2, 2);
        p.setClipPath(clip);
        p.fillRect(rect(), m_rest);
        double x = 0;
        for (const auto &part : m_parts) {
            const double w = qBound(0.0, part.fraction, 1.0) * width();
            if (w <= 0) continue;
            p.fillRect(QRectF(x, 0, w, height()), part.color);
            x += w + 1;
        }
    }

private:
    QVector<Part> m_parts;
    QColor m_rest;
};
