#pragma once
#include "WorkspaceSkin.h"
#include <QAbstractButton>
#include <QComboBox>
#include <QFontDatabase>
#include <QHBoxLayout>
#include <QIcon>
#include <QKeyEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPushButton>
#include <QStyleOptionComboBox>
#include <QStylePainter>
#include <QWidget>

// WhiteWh fork: the widgets of the dense New session view, drawn to the
// designer's handoff (design/new-session-dialog/HANDOFF.md in the fork's
// notes): a 520px window, a 50 | field | 30 | 148 grid, an agent rail with
// limits, and spaced row captions. Colors come from the skin when it names the
// handoff tokens, else from these defaults.
namespace NewSessionDense {

struct Tokens {
    QColor dialog, field, selected, segOn, btnNeutral, disabled, border, borderField, borderNeutral, track,
        text, text2, text3, muted, label, dim, accent, onAccent, warn, danger, folder;
};

inline QColor pick(const WorkspaceSkin::Skin &skin, bool dark, const QString &name, const char *darkValue, const char *lightValue)
{
    const auto value = skin.named(dark, name);
    return QColor(value.isEmpty() ? QString::fromLatin1(dark ? darkValue : lightValue) : value);
}

inline Tokens tokens(const WorkspaceSkin::Skin &skin, bool dark)
{
    const auto c = [&](const char *name, const char *d, const char *l) { return pick(skin, dark, QString::fromLatin1(name), d, l); };
    return {c("dialog", "#1b1f26", "#ffffff"), c("field", "#151920", "#f5f7f9"), c("selectedAgent", "#1f3a31", "#e0f0e9"),
            c("segmentOn", "#26443a", "#d8eee5"), c("buttonNeutral", "#232830", "#edf2f5"), c("disabled", "#2a3038", "#e4e8ec"),
            c("border", "#2c323b", "#dce2e8"), c("fieldBorder", "#39404a", "#c9d1d9"), c("neutralBorder", "#4a525d", "#aeb8c2"),
            c("track", "#2c323b", "#dce2e8"), c("foreground", "#e6e8eb", "#1a2733"), c("text2", "#c4cad1", "#334250"),
            c("text3", "#aab1ba", "#4d5c6b"), c("muted", "#9aa3ad", "#627082"), c("label", "#8b939e", "#6b7785"),
            c("dim", "#6f7883", "#8d97a2"), c("accent", "#7dd3b0", "#167357"), c("accentText", "#0f1a16", "#ffffff"),
            c("warn", "#e8a75a", "#a66000"), c("danger", "#ef7d6d", "#b3261e"), c("folder", "#7fb6e0", "#2f6f9f")};
}

inline QFont mono(int pixels, int weight = QFont::Normal)
{
    auto font = QFontDatabase::systemFont(QFontDatabase::FixedFont);
    font.setFamilies({QStringLiteral("JetBrains Mono"), QStringLiteral("Cascadia Mono"), QStringLiteral("Consolas"),
                      QStringLiteral("SF Mono"), QStringLiteral("Menlo"), QStringLiteral("DejaVu Sans Mono"), font.family()});
    font.setStyleHint(QFont::Monospace);
    font.setPixelSize(pixels);
    font.setWeight(QFont::Weight(weight));
    return font;
}

// A row caption: its letters spread across 40px, then 10px before the field.
class Caption : public QWidget {
public:
    explicit Caption(const QString &text, QColor color, QWidget *parent = nullptr) : QWidget(parent), m_text(text), m_color(color)
    {
        setObjectName("denseCaption");
        setAccessibleName(text);
        auto f = font(); f.setPixelSize(12); setFont(f);
        setFixedWidth(50);
        setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Preferred);
    }
    QSize sizeHint() const override { return {50, fontMetrics().height()}; }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.setPen(m_color);
        const auto metrics = fontMetrics();
        const int count = m_text.size();
        int letters = 0;
        for (const auto ch : m_text) letters += metrics.horizontalAdvance(ch);
        const double gap = count > 1 ? qMax(0.0, (40.0 - letters) / (count - 1)) : 0;
        double x = 0;
        const int y = (height() + metrics.ascent() - metrics.descent()) / 2;
        for (const auto ch : m_text) {
            p.drawText(QPointF(x, y), QString(ch));
            x += metrics.horizontalAdvance(ch) + gap;
        }
    }

private:
    QString m_text;
    QColor m_color;
};

// The primary button with its shortcut drawn at the right.
class HintButton : public QPushButton {
public:
    using QPushButton::QPushButton;
    void setHint(const QString &hint) { m_hint = hint; update(); }
    QSize sizeHint() const override
    {
        auto size = QPushButton::sizeHint();
        if (!m_hint.isEmpty()) size.rwidth() += QFontMetrics(mono(10)).horizontalAdvance(m_hint) + 8;
        return size;
    }

protected:
    void paintEvent(QPaintEvent *event) override
    {
        QPushButton::paintEvent(event);
        if (m_hint.isEmpty()) return;
        QPainter p(this);
        p.setFont(mono(10));
        p.setOpacity(0.65);
        p.setPen(palette().color(QPalette::ButtonText));
        p.drawText(rect().adjusted(0, 0, -14, 0), Qt::AlignRight | Qt::AlignVCenter, m_hint);
    }

private:
    QString m_hint;
};

// The working folder as its tail: "…\parent\" dimmed, then the folder itself.
class PathCombo : public QComboBox {
public:
    using QComboBox::QComboBox;
    void setTail(int pathRole, QColor head, QColor leaf)
    {
        m_role = pathRole; m_head = head; m_leaf = leaf; update();
    }

protected:
    void paintEvent(QPaintEvent *event) override
    {
        if (m_role < 0) { QComboBox::paintEvent(event); return; }
        QStylePainter p(this);
        QStyleOptionComboBox option;
        initStyleOption(&option);
        const auto path = currentIndex() < 0 ? QString() : itemData(currentIndex(), m_role).toString();
        option.currentText.clear();
        option.currentIcon = {};
        p.drawComplexControl(QStyle::CC_ComboBox, option);
        auto field = style()->subControlRect(QStyle::CC_ComboBox, &option, QStyle::SC_ComboBoxEditField, this).adjusted(2, 0, -2, 0);
        if (path.isEmpty()) {
            p.setPen(m_head);
            p.drawText(field, Qt::AlignLeft | Qt::AlignVCenter, placeholderText());
            return;
        }
        const auto separator = path.contains('\\') ? QChar('\\') : QChar('/');
        auto parts = QDir::fromNativeSeparators(path).split('/', Qt::SkipEmptyParts);
        const auto leafName = parts.isEmpty() ? path : parts.takeLast();
        const auto parent = parts.isEmpty() ? QString() : parts.takeLast();
        const auto head = QStringLiteral("…") + separator + (parent.isEmpty() ? QString() : parent + separator);
        auto leafFont = mono(12, QFont::Medium);
        const int leafWidth = QFontMetrics(leafFont).horizontalAdvance(leafName);
        const auto headFont = mono(12);
        const auto shownHead = QFontMetrics(headFont).elidedText(head, Qt::ElideLeft, qMax(0, field.width() - leafWidth));
        const int headWidth = QFontMetrics(headFont).horizontalAdvance(shownHead);
        p.setFont(headFont); p.setPen(m_head);
        p.drawText(field, Qt::AlignLeft | Qt::AlignVCenter, shownHead);
        p.setFont(leafFont); p.setPen(m_leaf);
        p.drawText(field.adjusted(headWidth, 0, 0, 0), Qt::AlignLeft | Qt::AlignVCenter,
                   QFontMetrics(leafFont).elidedText(leafName, Qt::ElideRight, qMax(0, field.width() - headWidth)));
    }

private:
    int m_role = -1;
    QColor m_head, m_leaf;
};

// One agent of the rail and what its button shows (handoff section 4).
struct Agent {
    QString key, name, kind; // kind: subscription, balance, local or terminal
    QIcon icon;
    bool connected = true, busy = false;
    double fiveHour = -1, week = -1; // percent used
    QString reset, balance, tooltip;
    double balanceValue = -1;
    bool narrow() const { return kind == QLatin1String("local") || kind == QLatin1String("terminal"); }
};

inline QColor usageColor(double used, const Tokens &t) { return used >= 85 ? t.danger : used >= 65 ? t.warn : t.accent; }

class AgentButton : public QAbstractButton {
public:
    AgentButton(const Agent &agent, const Tokens &tokens, QWidget *parent = nullptr) : QAbstractButton(parent), m_tokens(tokens)
    {
        setObjectName("denseAgent");
        setCheckable(true);
        setFocusPolicy(Qt::TabFocus);
        setCursor(Qt::PointingHandCursor);
        setAgent(agent);
    }
    void setAgent(const Agent &agent)
    {
        m_agent = agent;
        setProperty("agent", agent.key);
        setToolTip(agent.tooltip);
        setAccessibleName(agent.tooltip.isEmpty() ? agent.name : QString(agent.tooltip).replace('\n', QStringLiteral(" · ")));
        setFixedHeight(34);
        if (agent.narrow()) setFixedWidth(38); else { setMinimumWidth(0); setMaximumWidth(QWIDGETSIZE_MAX); }
        setSizePolicy(agent.narrow() ? QSizePolicy::Fixed : QSizePolicy::Expanding, QSizePolicy::Fixed);
        update();
    }
    const Agent &agent() const { return m_agent; }
    QSize sizeHint() const override { return {m_agent.narrow() ? 38 : 76, 34}; }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        const QRectF box = QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5);
        if (isChecked()) {
            p.setPen(m_tokens.accent); p.setBrush(m_tokens.selected);
            p.drawRoundedRect(box, 6, 6);
        } else if (hasFocus()) {
            p.setPen(m_tokens.borderNeutral); p.setBrush(Qt::NoBrush);
            p.drawRoundedRect(box, 6, 6);
        }
        const int iconSize = m_agent.kind == QLatin1String("local") ? 18 : 20;
        const bool off = !m_agent.connected && !m_agent.narrow();
        const auto lines = this->lines();
        int contentWidth = iconSize;
        if (!lines.isEmpty()) contentWidth += 4 + blockWidth(lines);
        int x = (width() - contentWidth) / 2;
        const auto pixmap = m_agent.icon.pixmap(QSize(iconSize, iconSize), devicePixelRatioF(), off ? QIcon::Disabled : QIcon::Normal);
        p.save();
        if (off) p.setOpacity(0.45);
        p.drawPixmap(QRect(x, (height() - iconSize) / 2, iconSize, iconSize), pixmap);
        p.restore();
        if (lines.isEmpty()) return;
        x += iconSize + 4;
        if (m_agent.kind == QLatin1String("subscription") && m_agent.connected) {
            // [bar][percent] for 5h and week, then the 5h reset: 8px rows, 1px apart.
            p.setFont(mono(8));
            const int top = (height() - 26) / 2;
            const double values[2] = {m_agent.fiveHour, m_agent.week};
            for (int row = 0; row < 2; ++row) {
                const int y = top + row * 9;
                p.fillRect(QRect(x, y + 3, 12, 2), m_tokens.track);
                if (values[row] >= 0) {
                    const auto color = usageColor(values[row], m_tokens);
                    p.fillRect(QRectF(x, y + 3, 12 * qBound(0.0, values[row], 100.0) / 100.0, 2), color);
                    p.setPen(values[row] < 65 ? m_tokens.text3 : color);
                    p.drawText(QRect(x + 14, y, 40, 8), Qt::AlignLeft | Qt::AlignVCenter, QString::number(qRound(values[row])) + '%');
                } else {
                    p.setPen(m_tokens.dim);
                    p.drawText(QRect(x + 14, y, 40, 8), Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("—"));
                }
            }
            p.setPen(m_tokens.text3);
            p.drawText(QRect(x, top + 18, 60, 8), Qt::AlignLeft | Qt::AlignVCenter, m_agent.reset);
            return;
        }
        // Two monospace lines: a balance, or why the account cannot start yet.
        const int top = (height() - 18) / 2;
        for (int row = 0; row < lines.size(); ++row) {
            const auto &[text, color, font] = lines[row];
            p.setFont(font); p.setPen(color);
            p.drawText(QRect(x, top + row * 10, 60, 9), Qt::AlignLeft | Qt::AlignVCenter, text);
        }
    }

private:
    struct Line { QString text; QColor color; QFont font; };
    QList<Line> lines() const
    {
        if (m_agent.narrow()) return {};
        if (m_agent.kind == QLatin1String("subscription") && m_agent.connected) return {{m_agent.reset, m_tokens.text3, mono(8)}};
        if (m_agent.kind == QLatin1String("balance") && m_agent.connected) {
            const auto color = m_agent.balanceValue < 0 ? m_tokens.text2 : m_agent.balanceValue < 2 ? m_tokens.danger : m_agent.balanceValue < 5 ? m_tokens.warn : m_tokens.text2;
            return {{m_agent.balance.isEmpty() ? QStringLiteral("key") : m_agent.balance, color, mono(9, QFont::Medium)}, {QStringLiteral("bal"), m_tokens.dim, mono(8)}};
        }
        const bool balance = m_agent.kind == QLatin1String("balance");
        return {{balance ? QStringLiteral("no key") : QStringLiteral("offline"), m_tokens.dim, mono(8)},
                {m_agent.busy ? QStringLiteral("···") : balance ? QStringLiteral("+ add") : QStringLiteral("sign in"), m_agent.busy ? m_tokens.label : m_tokens.warn, mono(8)}};
    }
    int blockWidth(const QList<Line> &lines) const
    {
        if (m_agent.kind == QLatin1String("subscription") && m_agent.connected)
            return qMax(14 + QFontMetrics(mono(8)).horizontalAdvance(QStringLiteral("100%")), QFontMetrics(mono(8)).horizontalAdvance(m_agent.reset));
        int widest = 0;
        for (const auto &line : lines) widest = qMax(widest, QFontMetrics(line.font).horizontalAdvance(line.text));
        return widest;
    }
    Agent m_agent;
    Tokens m_tokens;
};

// The rail: one bordered field whose selected button's frame lies on its border.
class AgentRail : public QWidget {
public:
    explicit AgentRail(const Tokens &tokens, QWidget *parent = nullptr) : QWidget(parent), m_tokens(tokens)
    {
        setObjectName("denseAgents");
        setAccessibleName(QObject::tr("Agent"));
        setFixedHeight(34);
        auto *row = new QHBoxLayout(this);
        row->setContentsMargins(0, 0, 0, 0);
        row->setSpacing(0);
    }
    std::function<void(const QString &)> selected;
    void setAgents(const QList<Agent> &agents, const QString &current)
    {
        QStringList keys;
        for (const auto &agent : agents) keys << agent.key;
        if (keys != m_keys) {
            qDeleteAll(m_buttons); m_buttons.clear();
            for (const auto &agent : agents) {
                auto *button = new AgentButton(agent, m_tokens, this);
                layout()->addWidget(button);
                connect(button, &QAbstractButton::clicked, this, [this, key = agent.key] { if (selected) selected(key); });
                button->installEventFilter(this);
                m_buttons << button;
            }
            m_keys = keys;
        }
        for (int i = 0; i < m_buttons.size(); ++i) {
            m_buttons[i]->setAgent(agents[i]);
            m_buttons[i]->setChecked(agents[i].key == current);
        }
    }
    QList<AgentButton *> buttons() const { return m_buttons; }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        p.setPen(m_tokens.borderField); p.setBrush(m_tokens.field);
        p.drawRoundedRect(QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5), 6, 6);
    }
    // Left and right move the choice, as in a radio group.
    bool eventFilter(QObject *object, QEvent *event) override
    {
        if (event->type() == QEvent::KeyPress) {
            const int key = static_cast<QKeyEvent *>(event)->key();
            const int at = m_buttons.indexOf(static_cast<AgentButton *>(object));
            if (at >= 0 && (key == Qt::Key_Left || key == Qt::Key_Right) && selected) {
                const int next = qBound(0, at + (key == Qt::Key_Right ? 1 : -1), int(m_buttons.size()) - 1);
                selected(m_keys[next]);
                m_buttons[next]->setFocus(Qt::TabFocusReason);
                return true;
            }
        }
        return QWidget::eventFilter(object, event);
    }

private:
    Tokens m_tokens;
    QStringList m_keys;
    QList<AgentButton *> m_buttons;
};

// The line under the rail: a title, then details, elided at the end.
class InfoLine : public QWidget {
public:
    explicit InfoLine(const Tokens &tokens, QWidget *parent = nullptr) : QWidget(parent), m_tokens(tokens)
    {
        setObjectName("denseInfo");
        auto f = font(); f.setPixelSize(10); setFont(f);
        setMinimumHeight(20);
        setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    }
    void setText(const QString &title, const QString &details, bool warn)
    {
        m_title = title; m_details = details; m_warn = warn;
        setAccessibleName(details.isEmpty() ? title : title + QStringLiteral(" · ") + details);
        setToolTip(accessibleName());
        update();
    }
    QString text() const { return accessibleName(); }
    QSize sizeHint() const override { return {200, 20}; }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        auto bold = font(); bold.setWeight(QFont::Medium);
        const QFontMetrics boldMetrics(bold), metrics(font());
        const int titleWidth = qMin(width(), boldMetrics.horizontalAdvance(m_title));
        p.setFont(bold); p.setPen(m_warn ? m_tokens.warn : m_tokens.text);
        p.drawText(QRect(0, 0, titleWidth, height()), Qt::AlignLeft | Qt::AlignVCenter, boldMetrics.elidedText(m_title, Qt::ElideRight, width()));
        if (m_details.isEmpty() || titleWidth >= width()) return;
        p.setFont(font()); p.setPen(m_tokens.label);
        p.drawText(QRect(titleWidth, 0, width() - titleWidth, height()), Qt::AlignLeft | Qt::AlignVCenter,
                   metrics.elidedText(QStringLiteral(" · ") + m_details, Qt::ElideRight, width() - titleWidth));
    }

private:
    Tokens m_tokens;
    QString m_title, m_details;
    bool m_warn = false;
};
}
