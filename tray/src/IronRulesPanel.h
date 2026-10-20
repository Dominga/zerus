#pragma once
#include "IronRules.h"
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QVBoxLayout>
#include <functional>

// WhiteWh fork: the iron rules of the selected session's project, in Details.
// Rules are numbered; each can be removed, and a new one is added from the
// field. Every message Zerus sends to an agent of the project carries them.
class IronRulesPanel : public QWidget {
public:
    explicit IronRulesPanel(QWidget *parent = nullptr) : QWidget(parent)
    {
        setObjectName("ironRulesPanel");
        setAttribute(Qt::WA_StyledBackground, true);
        auto *layout = new QVBoxLayout(this);
        layout->setContentsMargins(8, 6, 8, 8);
        layout->setSpacing(5);
        auto *head = new QHBoxLayout;
        auto *title = new QLabel(tr("Iron rules"));
        title->setObjectName("detailsIdentityTitle");
        head->addWidget(title);
        m_count = new QLabel;
        m_count->setObjectName("ironRulesCount");
        head->addWidget(m_count, 1);
        layout->addLayout(head);
        m_list = new QVBoxLayout;
        m_list->setSpacing(3);
        layout->addLayout(m_list);
        auto *add = new QHBoxLayout;
        add->setSpacing(6);
        m_field = new QLineEdit;
        m_field->setObjectName("ironRuleField");
        m_field->setPlaceholderText(tr("Add a rule…"));
        m_field->setAccessibleName(tr("New iron rule"));
        m_field->setMaxLength(IronRules::MaxLength);
        add->addWidget(m_field, 1);
        m_add = new QPushButton(tr("Add"));
        m_add->setObjectName("addIronRule");
        m_add->setAutoDefault(false);
        add->addWidget(m_add);
        layout->addLayout(add);
        m_hint = new QLabel(tr("Sent at the start of every message to this project's agents."));
        m_hint->setObjectName("ironRulesHint");
        m_hint->setWordWrap(true);
        layout->addWidget(m_hint);
        connect(m_add, &QPushButton::clicked, this, &IronRulesPanel::addRule);
        connect(m_field, &QLineEdit::returnPressed, this, &IronRulesPanel::addRule);
        connect(m_field, &QLineEdit::textChanged, this, [this] { m_add->setEnabled(canAdd()); });
        render();
    }
    // Called when the rules change, with their new count.
    std::function<void(int)> changed;
    void setProject(const QString &project, const QString &name)
    {
        if (project == m_project) return;
        m_project = project; m_name = name;
        render();
    }
    QStringList rules() const { return IronRules::rules(m_project); }

private:
    bool canAdd() const { return !m_project.isEmpty() && !m_field->text().trimmed().isEmpty() && rules().size() < IronRules::MaxRules; }
    void addRule()
    {
        if (!canAdd()) return;
        IronRules::setRules(m_project, rules() << m_field->text());
        m_field->clear();
        render();
        if (changed) changed(rules().size());
    }
    void removeRule(int at)
    {
        auto list = rules();
        if (at < 0 || at >= list.size()) return;
        list.removeAt(at);
        IronRules::setRules(m_project, list);
        render();
        if (changed) changed(list.size());
    }
    void render()
    {
        while (auto *item = m_list->takeAt(0)) { delete item->widget(); delete item; }
        const auto list = rules();
        setVisible(!m_project.isEmpty());
        m_count->setText(list.isEmpty() ? QString() : QString::number(list.size()));
        for (int at = 0; at < list.size(); ++at) {
            auto *row = new QWidget; row->setObjectName("ironRule");
            auto *line = new QHBoxLayout(row); line->setContentsMargins(0, 0, 0, 0); line->setSpacing(6);
            auto *number = new QLabel(QString::number(at + 1) + '.'); number->setObjectName("ironRuleNumber");
            number->setAlignment(Qt::AlignTop | Qt::AlignRight); number->setFixedWidth(18);
            line->addWidget(number);
            auto *text = new QLabel(list[at]); text->setObjectName("ironRuleText");
            text->setWordWrap(true); text->setTextFormat(Qt::PlainText); text->setTextInteractionFlags(Qt::TextSelectableByMouse);
            line->addWidget(text, 1);
            auto *remove = new QPushButton(QStringLiteral("×")); remove->setObjectName("removeIronRule");
            remove->setFixedSize(22, 22); remove->setAutoDefault(false);
            remove->setToolTip(tr("Remove this rule")); remove->setAccessibleName(tr("Remove rule %1").arg(at + 1));
            connect(remove, &QPushButton::clicked, this, [this, at] { removeRule(at); });
            line->addWidget(remove, 0, Qt::AlignTop);
            m_list->addWidget(row);
        }
        m_hint->setText(list.isEmpty() ? tr("No rules for %1 yet. Rules are sent at the start of every message to its agents.").arg(m_name)
                                       : tr("Sent at the start of every message to %1's agents.").arg(m_name));
        m_add->setEnabled(canAdd());
        m_field->setEnabled(!m_project.isEmpty() && list.size() < IronRules::MaxRules);
    }
    QString m_project, m_name;
    QLabel *m_count, *m_hint;
    QVBoxLayout *m_list;
    QLineEdit *m_field;
    QPushButton *m_add;
};
