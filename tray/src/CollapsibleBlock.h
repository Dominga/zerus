#pragma once
#include <QHBoxLayout>
#include <QLabel>
#include <QPushButton>
#include <QSettings>
#include <QVBoxLayout>
#include <QWidget>
#include <functional>

#include "Settings.h"
#include "WorkspaceIcons.h"
#include "WorkspaceSkin.h"

// WhiteWh fork, U25 (spec 1.5): the one collapsible block of the new UI. The
// header is a button: a chevron (right collapsed, down expanded), the bold
// title and an optional note such as a count. Actions added to trailing() stay
// right of it in both states. Collapsed, the body hides and an optional
// summary (a count, roles, a task chain) shows in a dashed box that expands
// the block when clicked. The state is kept in QSettings under
// "ui/collapsed/<key>" (no key keeps none). The header's checked state and its
// "expanded" property are aria-expanded; its accessible description says
// "Expanded" or "Collapsed". Colors are the skin's design tokens (setDark).
class CollapsibleBlock : public QWidget {
    // A push button sized by the labels it lays out (chevron, title, note).
    class Header : public QPushButton {
    public:
        QSize sizeHint() const override { return layout() ? layout()->sizeHint() : QPushButton::sizeHint(); }
        QSize minimumSizeHint() const override { return sizeHint(); }
    };

public:
    CollapsibleBlock(const QString &key, const QString &title, bool expandedByDefault = true, QWidget *parent = nullptr)
        : QWidget(parent), m_key(key)
    {
        setObjectName("collapsibleBlock");
        auto *v = new QVBoxLayout(this); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
        auto *head = new QHBoxLayout; head->setContentsMargins(0, 0, 0, 0); head->setSpacing(6);
        m_header = new Header; m_header->setObjectName("collapsibleHeader");
        m_header->setCheckable(true); m_header->setCursor(Qt::PointingHandCursor);
        m_header->setSizePolicy(QSizePolicy::Maximum, QSizePolicy::Fixed);
        auto *inside = new QHBoxLayout(m_header); inside->setContentsMargins(4, 2, 6, 2); inside->setSpacing(6);
        m_chevron = new QLabel; m_chevron->setObjectName("collapsibleChevron"); m_chevron->setFixedSize(10, 10);
        m_title = new QLabel(title); m_title->setObjectName("collapsibleTitle");
        m_note = new QLabel; m_note->setObjectName("collapsibleNote"); m_note->hide();
        for (auto *label : {m_chevron, m_title, m_note}) { label->setAttribute(Qt::WA_TransparentForMouseEvents); inside->addWidget(label); }
        head->addWidget(m_header);
        head->addStretch(1);
        m_trailing = new QHBoxLayout; m_trailing->setContentsMargins(0, 0, 0, 0); m_trailing->setSpacing(6);
        head->addLayout(m_trailing);
        v->addLayout(head);
        m_summary = new QPushButton; m_summary->setObjectName("collapsibleSummary");
        m_summary->setCursor(Qt::PointingHandCursor); m_summary->hide();
        v->addWidget(m_summary);
        m_bodyWidget = new QWidget; m_bodyWidget->setObjectName("collapsibleBody");
        m_body = new QVBoxLayout(m_bodyWidget); m_body->setContentsMargins(0, 0, 0, 0); m_body->setSpacing(6);
        v->addWidget(m_bodyWidget);
        m_expanded = m_key.isEmpty() ? expandedByDefault : Settings::value(settingsKey(m_key), expandedByDefault).toBool();
        QObject::connect(m_header, &QPushButton::clicked, this, [this] { setExpanded(!m_expanded); });
        QObject::connect(m_summary, &QPushButton::clicked, this, [this] { setExpanded(true); });
        setDark(true);
    }

    static QString settingsKey(const QString &key) { return QStringLiteral("ui/collapsed/") + key; }

    // The content, shown only while expanded.
    QVBoxLayout *body() const { return m_body; }
    // Right of the header in both states: actions such as "+ Folder".
    QHBoxLayout *trailing() const { return m_trailing; }
    QPushButton *header() const { return m_header; }
    void setTitle(const QString &title) { m_title->setText(title); refresh(); m_header->updateGeometry(); }
    // After the title in both states, dim: a count such as "3".
    void setNote(const QString &note) { m_note->setText(note); m_note->setVisible(!note.isEmpty()); m_header->updateGeometry(); }
    // Under the header while collapsed; empty shows nothing.
    void setSummary(const QString &summary) { m_summary->setText(summary); refresh(); }
    bool isExpanded() const { return m_expanded; }
    // Remembers the state under the block's key; `expandedChanged` runs on a change.
    void setExpanded(bool expanded)
    {
        if (expanded == m_expanded) return;
        m_expanded = expanded;
        if (!m_key.isEmpty()) Settings::set(settingsKey(m_key), expanded);
        refresh();
        if (expandedChanged) expandedChanged(expanded);
    }
    std::function<void(bool)> expandedChanged;

    void setDark(bool dark)
    {
        m_dark = dark;
        const auto &skin = WorkspaceSkin::active();
        const auto t = [&skin, dark](const char *name) { return WorkspaceSkin::color(skin, dark, QString::fromLatin1(name)).name(); };
        // The prototype's .bxh header and .csum summary.
        setStyleSheet(QStringLiteral(
            "QPushButton#collapsibleHeader{border:0;background:transparent;border-radius:6px;padding:0;text-align:left;}"
            "QPushButton#collapsibleHeader:hover{background:%1;}"
            "QPushButton#collapsibleHeader:focus{outline:0;}"
            "QLabel#collapsibleTitle{color:%2;font-size:12px;font-weight:600;background:transparent;}"
            "QLabel#collapsibleNote{color:%3;font-size:12px;background:transparent;}"
            "QPushButton#collapsibleSummary{border:1px dashed %4;border-radius:8px;background:transparent;color:%5;"
            "padding:6px 9px;text-align:left;font-size:11.5px;}"
            "QPushButton#collapsibleSummary:hover{border-color:%6;}")
            .arg(t("hover"), t("fg"), t("dim"), t("border"), t("text2"), t("text3")));
        m_chevronColor = WorkspaceSkin::color(skin, dark, QStringLiteral("muted"));
        refresh();
    }

private:
    void refresh()
    {
        m_header->setChecked(m_expanded);
        m_header->setProperty("expanded", m_expanded);
        m_header->setAccessibleName(m_title->text());
        m_header->setAccessibleDescription(m_expanded ? QStringLiteral("Expanded") : QStringLiteral("Collapsed"));
        m_header->setToolTip(m_expanded ? QStringLiteral("Collapse") : QStringLiteral("Expand"));
        m_chevron->setPixmap(workspaceIcon(m_expanded ? QStringLiteral("chevron-down") : QStringLiteral("chevron-right"), m_chevronColor)
                                 .pixmap(10, 10));
        m_bodyWidget->setVisible(m_expanded);
        m_summary->setVisible(!m_expanded && !m_summary->text().isEmpty());
    }

    QString m_key;
    bool m_expanded = true, m_dark = true;
    QColor m_chevronColor;
    QPushButton *m_header = nullptr, *m_summary = nullptr;
    QLabel *m_chevron = nullptr, *m_title = nullptr, *m_note = nullptr;
    QHBoxLayout *m_trailing = nullptr;
    QWidget *m_bodyWidget = nullptr;
    QVBoxLayout *m_body = nullptr;
};
