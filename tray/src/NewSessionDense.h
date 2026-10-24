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
#include <functional>

// The widgets of the dense New session view, drawn to a designer's handoff:
// a 520px window, a 50 | field | 30 | 148 grid, an agent rail with
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

// One account of a subscription agent (handoff section 4, several accounts).
struct Account {
    int row = -1; // in the dialog's account list
    QString email;
    bool connected = true;
    double fiveHour = -1, week = -1; // percent used
    QString reset;
};

// One agent of the rail and what its button shows (handoff section 4).
struct Agent {
    QString key, name, kind; // kind: subscription, balance, local or terminal
    QIcon icon;
    bool connected = true, busy = false; // connected: the chosen account
    double fiveHour = -1, week = -1; // percent used
    QString reset, balance, tooltip;
    double balanceValue = -1;
    // A subscription agent's accounts and the one the session uses.
    QList<Account> accounts;
    int chosen = -1;
    bool narrow() const { return kind == QLatin1String("local") || kind == QLatin1String("terminal"); }
    bool multi() const { return accounts.size() > 1; }
};

inline QColor usageColor(double used, const Tokens &t) { return used >= 85 ? t.danger : used >= 65 ? t.warn : t.accent; }
inline QColor usageText(double used, const Tokens &t) { return used >= 65 ? usageColor(used, t) : t.text3; }

// "5h ▬ 15%": a dim caption, a 12x2 bar of the share used and the percent.
inline int miniWidth(const QString &caption, double used, const QFont &font)
{
    const QFontMetrics metrics(font);
    return metrics.horizontalAdvance(caption) + 3 + 12 + 3
        + metrics.horizontalAdvance(used >= 0 ? QString::number(qRound(used)) + '%' : QStringLiteral("—"));
}
inline int drawMini(QPainter &p, int x, const QRect &line, const QString &caption, double used, const Tokens &t, const QFont &font)
{
    const QFontMetrics metrics(font);
    p.setFont(font);
    p.setPen(t.dim);
    p.drawText(QRect(x, line.y(), metrics.horizontalAdvance(caption) + 1, line.height()), Qt::AlignLeft | Qt::AlignVCenter, caption);
    int at = x + metrics.horizontalAdvance(caption) + 3;
    const QRectF bar(at, line.center().y() - 0.5, 12, 2);
    p.setPen(Qt::NoPen);
    p.setBrush(t.track);
    p.drawRect(bar);
    if (used >= 0) {
        p.setBrush(usageColor(used, t));
        p.drawRect(QRectF(bar.x(), bar.y(), qMax(1.0, 12 * qBound(0.0, used, 100.0) / 100.0), 2));
    }
    at += 12 + 3;
    const auto text = used >= 0 ? QString::number(qRound(used)) + '%' : QStringLiteral("—");
    p.setPen(used >= 0 ? usageText(used, t) : t.dim);
    p.drawText(QRect(at, line.y(), metrics.horizontalAdvance(text) + 1, line.height()), Qt::AlignLeft | Qt::AlignVCenter, text);
    return miniWidth(caption, used, font);
}

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
        setFixedHeight(kHeight);
        if (agent.narrow()) setFixedWidth(kNarrow); else { setMinimumWidth(0); setMaximumWidth(QWIDGETSIZE_MAX); }
        setSizePolicy(agent.narrow() ? QSizePolicy::Fixed : QSizePolicy::Expanding, QSizePolicy::Fixed);
        update();
    }
    const Agent &agent() const { return m_agent; }
    QSize sizeHint() const override { return {m_agent.narrow() ? kNarrow : 92, kHeight}; }
    static constexpr int kHeight = 46, kNarrow = 46;

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
        const int iconSize = m_agent.kind == QLatin1String("local") ? 22 : 24;
        // An agent with several accounts never shows the offline state.
        const bool off = !m_agent.connected && !m_agent.narrow() && !m_agent.multi();
        const auto lines = this->lines();
        int contentWidth = iconSize;
        if (m_agent.multi()) contentWidth += 6 + multiWidth();
        else if (!lines.isEmpty()) contentWidth += 6 + blockWidth(lines);
        int x = (width() - contentWidth) / 2;
        const auto pixmap = m_agent.icon.pixmap(QSize(iconSize, iconSize), devicePixelRatioF(), off ? QIcon::Disabled : QIcon::Normal);
        p.save();
        if (off) p.setOpacity(0.45);
        p.drawPixmap(QRect(x, (height() - iconSize) / 2, iconSize, iconSize), pixmap);
        p.restore();
        if (m_agent.multi()) {
            // "N acc" over one dot per account, colored by its 5h use.
            x += iconSize + 6;
            const auto font = mono(11, QFont::Medium);
            const int top = (height() - (kLine + 3 + kDot)) / 2;
            p.setFont(font);
            p.setPen(m_tokens.text2);
            p.drawText(QRect(x, top, 80, kLine), Qt::AlignLeft | Qt::AlignVCenter, countText());
            p.setPen(Qt::NoPen);
            for (int i = 0; i < m_agent.accounts.size(); ++i) {
                const auto &account = m_agent.accounts[i];
                p.setBrush(!account.connected ? m_tokens.borderNeutral : account.fiveHour >= 0 ? usageColor(account.fiveHour, m_tokens) : m_tokens.dim);
                p.drawRoundedRect(QRectF(x + i * (kDot + 3), top + kLine + 3, kDot, kDot), kDot / 2.0, kDot / 2.0);
            }
            return;
        }
        if (lines.isEmpty()) return;
        x += iconSize + 6;
        if (m_agent.kind == QLatin1String("subscription") && m_agent.connected) {
            // [bar][percent] for 5h and week, then the 5h reset: 12px rows.
            p.setFont(mono(kMeter));
            const int top = (height() - 3 * kRow) / 2;
            const double values[2] = {m_agent.fiveHour, m_agent.week};
            for (int row = 0; row < 2; ++row) {
                const int y = top + row * kRow;
                const QRectF bar(x, y + (kRow - 3) / 2.0, kBar, 3);
                p.setPen(Qt::NoPen);
                p.setBrush(m_tokens.track);
                p.drawRoundedRect(bar, 1.5, 1.5);
                const QRect label(x + kBar + 4, y, 48, kRow);
                if (values[row] >= 0) {
                    const auto color = usageColor(values[row], m_tokens);
                    p.setBrush(color);
                    p.drawRoundedRect(QRectF(bar.x(), bar.y(), qMax(1.5, kBar * qBound(0.0, values[row], 100.0) / 100.0), 3), 1.5, 1.5);
                    p.setPen(values[row] < 65 ? m_tokens.text2 : color);
                    p.drawText(label, Qt::AlignLeft | Qt::AlignVCenter, QString::number(qRound(values[row])) + '%');
                } else {
                    p.setPen(m_tokens.dim);
                    p.drawText(label, Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("—"));
                }
            }
            p.setPen(m_tokens.text3);
            p.drawText(QRect(x, top + 2 * kRow, 80, kRow), Qt::AlignLeft | Qt::AlignVCenter, m_agent.reset);
            return;
        }
        // One or two monospace lines: a balance, or why the account cannot start yet.
        const int top = (height() - lines.size() * kLine) / 2;
        for (int row = 0; row < lines.size(); ++row) {
            const auto &[text, color, font] = lines[row];
            p.setFont(font); p.setPen(color);
            p.drawText(QRect(x, top + row * kLine, 80, kLine), Qt::AlignLeft | Qt::AlignVCenter, text);
        }
    }

private:
    static constexpr int kMeter = 10, kRow = 12, kBar = 18, kLine = 14, kDot = 5;
    struct Line { QString text; QColor color; QFont font; };
    QString countText() const { return QStringLiteral("%1 acc").arg(m_agent.accounts.size()); }
    int multiWidth() const
    {
        const int dots = int(m_agent.accounts.size()) * (kDot + 3) - 3;
        return qMax(QFontMetrics(mono(11, QFont::Medium)).horizontalAdvance(countText()), dots);
    }
    QList<Line> lines() const
    {
        if (m_agent.narrow()) return {};
        if (m_agent.kind == QLatin1String("subscription") && m_agent.connected) return {{m_agent.reset, m_tokens.text3, mono(kMeter)}};
        if (m_agent.kind == QLatin1String("balance") && m_agent.connected) {
            // A key whose balance is not known yet says only that it is there.
            if (m_agent.balance.isEmpty()) return {{QStringLiteral("key"), m_tokens.text2, mono(11, QFont::Medium)}};
            const auto color = m_agent.balanceValue < 2 ? m_tokens.danger : m_agent.balanceValue < 5 ? m_tokens.warn : m_tokens.text2;
            return {{m_agent.balance, color, mono(11, QFont::Medium)}, {QStringLiteral("bal"), m_tokens.dim, mono(kMeter)}};
        }
        const bool balance = m_agent.kind == QLatin1String("balance");
        return {{balance ? QStringLiteral("no key") : QStringLiteral("offline"), m_tokens.dim, mono(kMeter)},
                {m_agent.busy ? QStringLiteral("···") : balance ? QStringLiteral("+ add") : QStringLiteral("sign in"), m_agent.busy ? m_tokens.label : m_tokens.warn, mono(kMeter)}};
    }
    int blockWidth(const QList<Line> &lines) const
    {
        if (m_agent.kind == QLatin1String("subscription") && m_agent.connected)
            return qMax(kBar + 4 + QFontMetrics(mono(kMeter)).horizontalAdvance(QStringLiteral("100%")), QFontMetrics(mono(kMeter)).horizontalAdvance(m_agent.reset));
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
        setFixedHeight(AgentButton::kHeight);
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

// The line under the rail for an agent with several accounts: the chosen one
// with its limits, opening the list of all (handoff section 4).
class AccountChip : public QAbstractButton {
public:
    explicit AccountChip(const Tokens &tokens, QWidget *parent = nullptr) : QAbstractButton(parent), m_tokens(tokens)
    {
        setObjectName("denseAccountChip");
        setFixedHeight(20);
        setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
        setCursor(Qt::PointingHandCursor);
        setFocusPolicy(Qt::TabFocus);
        // A click on the chip while its list is open only closes the list.
        setAttribute(Qt::WA_NoMouseReplay);
    }
    void setAccounts(const QList<Account> &accounts, int chosen)
    {
        m_accounts = accounts;
        m_chosen = chosen >= 0 && chosen < accounts.size() ? chosen : -1;
        const auto email = m_chosen < 0 ? QString() : accounts[m_chosen].email;
        setAccessibleName(QObject::tr("Account: %1").arg(email));
        QStringList tip;
        for (const auto &account : accounts)
            tip << (account.connected ? QObject::tr("%1 — 5h %2% · wk %3%").arg(account.email).arg(qRound(qMax(0.0, account.fiveHour))).arg(qRound(qMax(0.0, account.week)))
                                      : QObject::tr("%1 — not signed in").arg(account.email));
        setToolTip(tip.join('\n'));
        update();
    }
    const QList<Account> &accounts() const { return m_accounts; }
    int chosen() const { return m_chosen; }
    void setOpen(bool open) { m_open = open; update(); }
    QSize sizeHint() const override { return {200, 20}; }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        p.setPen(m_open || hasFocus() ? m_tokens.accent : m_tokens.borderField);
        p.setBrush(m_tokens.field);
        p.drawRoundedRect(QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5), 4, 4);
        if (m_chosen < 0) return;
        const auto &account = m_accounts[m_chosen];
        auto count = font();
        count.setPixelSize(10);
        count.setWeight(QFont::Medium);
        const auto countText = QStringLiteral("%1 acc").arg(m_accounts.size());
        int x = 7;
        p.setFont(count);
        p.setPen(m_tokens.text2);
        p.drawText(QRect(x, 0, 60, height()), Qt::AlignLeft | Qt::AlignVCenter, countText);
        x += QFontMetrics(count).horizontalAdvance(countText) + 8;
        // The chevron, turned over while the list is open.
        const double cx = width() - 7 - 8, cy = height() / 2.0;
        QPainterPath chevron;
        const double up = m_open ? -1 : 1;
        chevron.moveTo(cx + 1.5, cy - 1.5 * up);
        chevron.lineTo(cx + 4, cy + 1 * up);
        chevron.lineTo(cx + 6.5, cy - 1.5 * up);
        p.setPen(QPen(m_tokens.label, 1.4, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        p.setBrush(Qt::NoBrush);
        p.drawPath(chevron);
        const auto mini = mono(9), email = mono(10);
        const QString absent = QObject::tr("not signed in");
        const int metersWidth = account.connected
            ? miniWidth(QStringLiteral("5h"), account.fiveHour, mini) + 6 + miniWidth(QStringLiteral("wk"), account.week, mini) + 6 + QFontMetrics(mini).horizontalAdvance(account.reset)
            : QFontMetrics(mini).horizontalAdvance(absent);
        const int meters = int(cx) - 8 - metersWidth;
        p.setFont(email);
        p.setPen(account.connected ? m_tokens.text : m_tokens.label);
        p.drawText(QRect(x, 0, qMax(0, meters - 8 - x), height()), Qt::AlignLeft | Qt::AlignVCenter,
                   QFontMetrics(email).elidedText(account.email, Qt::ElideRight, qMax(0, meters - 8 - x)));
        const QRect line(0, 0, width(), height());
        if (!account.connected) {
            p.setFont(mini);
            p.setPen(m_tokens.warn);
            p.drawText(QRect(meters, 0, metersWidth + 1, height()), Qt::AlignLeft | Qt::AlignVCenter, absent);
            return;
        }
        int at = meters + drawMini(p, meters, line, QStringLiteral("5h"), account.fiveHour, m_tokens, mini) + 6;
        at += drawMini(p, at, line, QStringLiteral("wk"), account.week, m_tokens, mini) + 6;
        p.setFont(mini);
        p.setPen(m_tokens.text3);
        p.drawText(QRect(at, 0, width() - at, height()), Qt::AlignLeft | Qt::AlignVCenter, account.reset);
    }

private:
    Tokens m_tokens;
    QList<Account> m_accounts;
    int m_chosen = -1;
    bool m_open = false;
};

// The accounts of one agent under the chip, floating over the form and past the
// window's edge: a check, the email, 5h, week and the 5h reset, then Add account.
// A choice, a click outside, Esc or another agent closes it.
class AccountList : public QWidget {
public:
    explicit AccountList(const Tokens &tokens, QWidget *parent = nullptr)
        : QWidget(parent, Qt::Popup | Qt::FramelessWindowHint | Qt::NoDropShadowWindowHint), m_tokens(tokens)
    {
        setObjectName("denseAccountList");
        setAccessibleName(QObject::tr("Accounts"));
        setAttribute(Qt::WA_TranslucentBackground);
        setMouseTracking(true);
        setFocusPolicy(Qt::StrongFocus);
    }
    std::function<void(int row)> chosen;
    std::function<void()> addAccount, closed;
    void open(const QList<Account> &accounts, int current, QWidget *anchor)
    {
        m_accounts = accounts;
        m_current = m_hover = current;
        resize(anchor->width() + 2 * kShadow, 2 * kShadow + 2 + 4 + int(accounts.size()) * kRow + kAdd);
        move(anchor->mapToGlobal(QPoint(-kShadow, anchor->height() + 3 - kShadow)));
        show();
        setFocus(Qt::PopupFocusReason);
    }
    int count() const { return int(m_accounts.size()); }

protected:
    QRect panel() const { return rect().adjusted(kShadow, kShadow, -kShadow, -kShadow); }
    QRect rowRect(int index) const
    {
        const auto box = panel();
        if (index < count()) return {box.left() + 3, box.top() + 3 + index * kRow, box.width() - 6, kRow};
        return {box.left() + 1, box.top() + 3 + count() * kRow + 1, box.width() - 2, kAdd};
    }
    int at(const QPoint &point) const
    {
        for (int index = 0; index <= count(); ++index)
            if (rowRect(index).contains(point)) return index;
        return -1;
    }
    void activate(int index)
    {
        if (index < 0) return;
        hide();
        if (index < count()) { if (chosen) chosen(m_accounts[index].row); }
        else if (addAccount) addAccount();
    }
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        // A soft shadow, 0 8px 24px in the handoff, drawn as fading frames.
        p.setPen(Qt::NoPen);
        for (int step = kShadow; step > 0; --step) {
            p.setBrush(QColor(0, 0, 0, 7));
            p.drawRoundedRect(QRectF(panel()).adjusted(-step + 1, -step / 2.0 + 2, step - 1, step + 2), 6 + step, 6 + step);
        }
        const auto box = panel();
        p.setPen(m_tokens.borderField);
        p.setBrush(m_tokens.field);
        p.drawRoundedRect(QRectF(box).adjusted(0.5, 0.5, -0.5, -0.5), 6, 6);
        const auto email = mono(10), mini = mono(9);
        for (int index = 0; index < count(); ++index) {
            const auto &account = m_accounts[index];
            const auto row = rowRect(index);
            if (index == m_current || index == m_hover) {
                p.setPen(Qt::NoPen);
                p.setBrush(index == m_current ? m_tokens.selected : m_tokens.btnNeutral);
                p.drawRoundedRect(row, 4, 4);
            }
            // 10px | email | 48px | 48px | 42px, 8px apart.
            const int left = row.left() + 6, right = row.right() - 6;
            const int reset = right - 42, week = reset - 8 - 48, five = week - 8 - 48, mail = left + 10 + 8;
            if (index == m_current) {
                QPainterPath check;
                const double cy = row.center().y() + 0.5;
                check.moveTo(left + 1.2, cy);
                check.lineTo(left + 3.4, cy + 2.2);
                check.lineTo(left + 8, cy - 2.6);
                p.setPen(QPen(m_tokens.accent, 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
                p.setBrush(Qt::NoBrush);
                p.drawPath(check);
            }
            p.setFont(email);
            p.setPen(account.connected ? m_tokens.text : m_tokens.label);
            p.drawText(QRect(mail, row.y(), qMax(0, five - 8 - mail), row.height()), Qt::AlignLeft | Qt::AlignVCenter,
                       QFontMetrics(email).elidedText(account.email, Qt::ElideRight, qMax(0, five - 8 - mail)));
            if (!account.connected) {
                p.setFont(mini);
                p.setPen(m_tokens.warn);
                p.drawText(QRect(five, row.y(), right - five, row.height()), Qt::AlignLeft | Qt::AlignVCenter, QObject::tr("not signed in"));
                continue;
            }
            drawMini(p, five, row, QStringLiteral("5h"), account.fiveHour, m_tokens, mini);
            drawMini(p, week, row, QStringLiteral("wk"), account.week, m_tokens, mini);
            p.setFont(mini);
            p.setPen(m_tokens.text3);
            p.drawText(QRect(reset, row.y(), 42, row.height()), Qt::AlignLeft | Qt::AlignVCenter, account.reset);
        }
        // Add account, under a rule.
        const auto add = rowRect(count());
        p.setPen(m_tokens.border);
        p.drawLine(add.left(), add.top(), add.right(), add.top());
        if (m_hover == count()) {
            p.setPen(Qt::NoPen);
            p.setBrush(m_tokens.btnNeutral);
            p.drawRoundedRect(QRectF(add).adjusted(2, 1, -2, -2), 4, 4);
        }
        const double cx = add.left() + 8 + 4.5, cy = add.center().y() + 0.5;
        p.setPen(QPen(m_hover == count() ? m_tokens.text : m_tokens.muted, 1.4, Qt::SolidLine, Qt::RoundCap));
        p.drawLine(QPointF(cx, cy - 3.5), QPointF(cx, cy + 3.5));
        p.drawLine(QPointF(cx - 3.5, cy), QPointF(cx + 3.5, cy));
        auto label = font();
        label.setPixelSize(10);
        p.setFont(label);
        p.drawText(QRect(add.left() + 8 + 9 + 8, add.y(), add.width(), add.height()), Qt::AlignLeft | Qt::AlignVCenter, QObject::tr("Add account"));
    }
    void mouseMoveEvent(QMouseEvent *event) override
    {
        const int index = at(event->position().toPoint());
        if (index != m_hover) { m_hover = index; update(); }
    }
    void leaveEvent(QEvent *) override { m_hover = -1; update(); }
    void mouseReleaseEvent(QMouseEvent *event) override
    {
        if (event->button() == Qt::LeftButton) activate(at(event->position().toPoint()));
    }
    void keyPressEvent(QKeyEvent *event) override
    {
        switch (event->key()) {
        case Qt::Key_Down: m_hover = qBound(0, m_hover + 1, count()); update(); return;
        case Qt::Key_Up: m_hover = qBound(0, m_hover < 0 ? count() : m_hover - 1, count()); update(); return;
        case Qt::Key_Return: case Qt::Key_Enter: case Qt::Key_Space: activate(m_hover); return;
        case Qt::Key_Escape: hide(); return;
        default: QWidget::keyPressEvent(event);
        }
    }
    void hideEvent(QHideEvent *event) override
    {
        QWidget::hideEvent(event);
        if (closed) closed();
    }

private:
    static constexpr int kShadow = 12, kRow = 22, kAdd = 20;
    Tokens m_tokens;
    QList<Account> m_accounts;
    int m_current = -1, m_hover = -1;
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
