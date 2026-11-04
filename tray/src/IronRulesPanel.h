#pragma once
#include "IronRules.h"
#include <QHBoxLayout>
#include <QComboBox>
#include <QPointer>
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
        title->setObjectName("inspectorPanelTitle");
        head->addWidget(title);
        m_count = new QLabel;
        m_count->setObjectName("ironRulesCount");
        head->addWidget(m_count, 1);
        auto *refresh = new QPushButton(tr("Refresh")); refresh->setObjectName("reloadIronRules"); refresh->setAutoDefault(false);
        head->addWidget(refresh); connect(refresh, &QPushButton::clicked, this, [this] { reload(); });
        layout->addLayout(head);
        m_scope = new QComboBox;
        m_scope->setObjectName("ironRulesScope");
        m_scope->addItem(tr("Project"), "project");
        m_scope->addItem(tr("Session additions"), "session");
        layout->addWidget(m_scope);
        connect(m_scope, &QComboBox::currentIndexChanged, this, [this] { reload(); });
        m_error = new QLabel; m_error->setWordWrap(true); m_error->setTextFormat(Qt::PlainText);
        m_error->setObjectName("ironRulesError"); layout->addWidget(m_error);
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
    void setBackend(IronRules::Transport send) { m_send = std::move(send); reload(); }
    void setContext(const QString &host, const QString &name, const QString &run, const QString &conversation)
    {
        if (m_host == host && m_session == name && m_run == run && m_conversation == conversation) return;
        m_host = host; m_session = name; m_run = run; m_conversation = conversation;
        if (name.isEmpty()) m_scope->setCurrentIndex(0);
        reload();
    }
    void setProject(const QString &project, const QString &name)
    {
        if (project == m_project) return;
        m_projectRules.clear(); m_project = project; m_name = name;
        reload();
    }
    QStringList projectRules() const { return m_send ? m_projectRules : IronRules::rules(m_project); }
    QStringList rules() const { return m_send ? m_rules : IronRules::rules(m_project); }

private:
    bool canAdd() const { return !m_loading && !m_failed && (m_scope->currentData() == "session" ? !m_session.isEmpty() : !m_project.isEmpty()) && !m_field->text().trimmed().isEmpty() && rules().size() < IronRules::MaxRules; }
    void addRule()
    {
        if (!canAdd()) return;
        saveRules(rules() << m_field->text());
    }
    void removeRule(int at)
    {
        auto list = rules();
        if (at < 0 || at >= list.size()) return;
        list.removeAt(at);
        saveRules(list);
    }
    QJsonObject identity() const
    {
        if (m_scope->currentData() == "session") return {{"scope", "session"}, {"name", m_session}, {"expected_run_id", m_run}, {"expected_conversation_id", m_conversation}};
        return {{"scope", "project"}, {"project", m_project}};
    }
    void reload()
    {
        const auto generation = ++m_generation;
        if (!m_send) { render(); return; }
        m_rules.clear(); m_loading = true; m_failed = false; m_error->clear(); render();
        if ((m_scope->currentData() == "session" && m_session.isEmpty()) || (m_scope->currentData() == "project" && m_project.isEmpty())) { m_loading = false; render(); return; }
        auto done = [guard = QPointer<IronRulesPanel>(this), generation](const QJsonObject &value, const QString &failure) {
            if (!guard || guard->m_generation != generation) return;
            const auto error = value.value("unsupported").toBool() ? tr("Update this computer's hgs to edit iron rules. Existing rules are kept.") : failure;
            guard->m_loading = false; guard->m_failed = !error.isEmpty(); guard->m_error->setText(error);
            if (error.isEmpty()) { guard->m_rules = IronRules::list(value.value("rules")); guard->m_revision = value.value("revision").toInteger(); if (guard->m_scope->currentData() == "project") guard->m_projectRules = guard->m_rules; }
            guard->render(); if (guard->changed) guard->changed(guard->rules().size());
        };
        if (m_scope->currentData() == "project") IronRules::loadProject(m_send, m_host, m_project, done);
        else m_send(m_host, "get", identity(), done);
    }
    void saveRules(const QStringList &list)
    {
        if (!m_send) { IronRules::setRules(m_project, list); m_field->clear(); render(); if (changed) changed(list.size()); return; }
        if (m_loading) return;
        auto input = identity(); input.insert("rules", QJsonArray::fromStringList(list));
        input.insert("expected_revision", m_revision);
        input.insert("request_id", QUuid::createUuid().toString(QUuid::WithoutBraces));
        m_loading = true; render(); const auto generation = m_generation;
        m_send(m_host, "set", input, [guard = QPointer<IronRulesPanel>(this), generation](const QJsonObject &value, const QString &failure) {
            if (!guard || guard->m_generation != generation) return;
            const auto error = value.value("unsupported").toBool() ? tr("Update this computer's hgs to edit iron rules. Existing rules are kept.") : failure;
            guard->m_loading = false; guard->m_error->setText(error);
            if (error.isEmpty()) { guard->m_rules = IronRules::list(value.value("rules")); guard->m_revision = value.value("revision").toInteger(); guard->m_field->clear(); if (guard->m_scope->currentData() == "project") guard->m_projectRules = guard->m_rules; }
            guard->render(); if (guard->changed) guard->changed(guard->rules().size());
        });
    }
    void render()
    {
        while (auto *item = m_list->takeAt(0)) { delete item->widget(); delete item; }
        const auto list = rules();
        setVisible(!m_project.isEmpty() || !m_session.isEmpty());
        m_scope->setVisible(!m_session.isEmpty());
        m_scope->setEnabled(!m_loading);
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
            remove->setEnabled(!m_loading && !m_failed); remove->setFixedSize(22, 22); remove->setAutoDefault(false);
            remove->setToolTip(tr("Remove this rule")); remove->setAccessibleName(tr("Remove rule %1").arg(at + 1));
            connect(remove, &QPushButton::clicked, this, [this, at] { removeRule(at); });
            line->addWidget(remove, 0, Qt::AlignTop);
            m_list->addWidget(row);
        }
        m_hint->setText(tr("Rules add to inherited project, team and member rules. Delivered at session start and each team task. Agents without startup delivery also receive them with messages."));
        m_add->setEnabled(canAdd());
        m_field->setEnabled(!m_loading && !m_failed && (m_scope->currentData() == "session" ? !m_session.isEmpty() : !m_project.isEmpty()) && list.size() < IronRules::MaxRules);
    }
    IronRules::Transport m_send;
    QStringList m_rules, m_projectRules;
    QString m_host, m_session, m_run, m_conversation;
    qint64 m_revision = 0;
    quint64 m_generation = 0;
    bool m_loading = false, m_failed = false;
    QComboBox *m_scope;
    QLabel *m_error;
    QString m_project, m_name;
    QLabel *m_count, *m_hint;
    QVBoxLayout *m_list;
    QLineEdit *m_field;
    QPushButton *m_add;
};
