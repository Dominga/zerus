#pragma once
#include "IronRules.h"
#include <QCheckBox>
#include <QFileDialog>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QJsonObject>
#include <QLabel>
#include <QMessageBox>
#include <QPointer>
#include <QPushButton>
#include <QUuid>
#include <QVBoxLayout>
#include <functional>

// Task 126 (owner, 2026-11-07): project memory on Projects. One switch turns
// RustCodeLore on for the whole project (all its folders); the store folder
// shows with Browse (an empty store takes the folder, one with records moves)
// and Use default folder. A move is confirmed first, then hgs copies the
// store, checks every file and only then switches; the old store is kept.
// Turning memory off keeps the store. hgs owns the settings
// (`hgs project config get|set|move --json`, project-memory-v1).
namespace ProjectMemory {

inline QString sizeText(qint64 bytes)
{
    if (bytes < 1024) return QObject::tr("%1 B").arg(bytes);
    if (bytes < 1024 * 1024) return QObject::tr("%1 KiB").arg(QString::number(bytes / 1024.0, 'f', 1));
    if (bytes < 1024LL * 1024 * 1024) return QObject::tr("%1 MiB").arg(QString::number(bytes / (1024.0 * 1024), 'f', 1));
    return QObject::tr("%1 GiB").arg(QString::number(bytes / (1024.0 * 1024 * 1024), 'f', 1));
}

// The status line: records, size and the index state.
inline QString statusText(const QJsonObject &view)
{
    const auto status = view.value("status").toObject();
    if (!status.value("exists").toBool()) return QObject::tr("No store yet: it is made when memory is turned on.");
    const auto index = status.value("index").toString();
    const auto state = index == "built" ? QObject::tr("index up to date") : index == "stale" ? QObject::tr("index out of date") : QObject::tr("no index yet");
    return QObject::tr("%1 records, %2, %3").arg(status.value("records").toInteger()).arg(sizeText(status.value("bytes").toInteger()), state);
}

// A write's reply carries its view; a read is the view.
inline QJsonObject viewOf(const QJsonObject &reply) { return reply.contains("view") ? reply.value("view").toObject() : reply; }

} // namespace ProjectMemory

class ProjectMemoryPanel : public QWidget {
public:
    explicit ProjectMemoryPanel(QWidget *parent = nullptr) : QWidget(parent)
    {
        setObjectName("projectMemoryPanel");
        auto *layout = new QVBoxLayout(this);
        layout->setContentsMargins(8, 6, 8, 8);
        layout->setSpacing(5);
        auto *head = new QHBoxLayout;
        m_switch = new QCheckBox(tr("Use RustCodeLore for this project"));
        m_switch->setObjectName("projectMemorySwitch");
        head->addWidget(m_switch, 1);
        auto *refresh = new QPushButton(tr("Refresh")); refresh->setObjectName("projectMemoryRefresh"); refresh->setAutoDefault(false);
        head->addWidget(refresh);
        layout->addLayout(head);
        auto *storeRow = new QHBoxLayout;
        auto *storeLabel = new QLabel(tr("Store")); storeRow->addWidget(storeLabel);
        m_store = new QLabel; m_store->setObjectName("projectMemoryStore"); m_store->setTextFormat(Qt::PlainText);
        m_store->setWordWrap(true); m_store->setTextInteractionFlags(Qt::TextSelectableByMouse);
        storeRow->addWidget(m_store, 1);
        layout->addLayout(storeRow);
        auto *actions = new QHBoxLayout;
        m_browse = new QPushButton(tr("Browse…")); m_browse->setObjectName("projectMemoryBrowse"); m_browse->setAutoDefault(false);
        m_default = new QPushButton(tr("Use default folder")); m_default->setObjectName("projectMemoryDefault"); m_default->setAutoDefault(false);
        actions->addWidget(m_browse); actions->addWidget(m_default); actions->addStretch(1);
        layout->addLayout(actions);
        m_status = new QLabel; m_status->setObjectName("projectMemoryStatus"); m_status->setWordWrap(true); m_status->setTextFormat(Qt::PlainText);
        layout->addWidget(m_status);
        m_notice = new QLabel; m_notice->setObjectName("projectMemoryNotice"); m_notice->setWordWrap(true); m_notice->setTextFormat(Qt::PlainText);
        layout->addWidget(m_notice);
        m_error = new QLabel; m_error->setObjectName("projectMemoryError"); m_error->setWordWrap(true); m_error->setTextFormat(Qt::PlainText);
        layout->addWidget(m_error);
        auto *hint = new QLabel(tr("Teams of this project get a shared workspace in this store. Turning memory off keeps the store."));
        hint->setObjectName("projectMemoryHint"); hint->setWordWrap(true);
        layout->addWidget(hint);
        connect(refresh, &QPushButton::clicked, this, [this] { reload(); });
        connect(m_switch, &QCheckBox::clicked, this, [this](bool on) {
            m_switch->setChecked(m_view.value("enabled").toBool());
            if (on != m_view.value("enabled").toBool()) write("set", {{"enabled", on}});
        });
        connect(m_browse, &QPushButton::clicked, this, &ProjectMemoryPanel::browse);
        connect(m_default, &QPushButton::clicked, this, &ProjectMemoryPanel::useDefault);
        pickFolder = [this](const QString &start) { return QFileDialog::getExistingDirectory(this, tr("Store folder"), start); };
        confirmMove = [this](const QString &from, const QString &to, qint64 records) {
            QMessageBox confirm(QMessageBox::Question, tr("Move the project memory?"),
                tr("Zerus copies the store with its %1 records from %2 to %3, checks every file, and only then uses the new folder. The old store is kept; delete it yourself once you no longer need it.").arg(records).arg(from, to),
                QMessageBox::Cancel, this);
            confirm.setObjectName("projectMemoryMoveConfirm"); confirm.setTextFormat(Qt::PlainText);
            auto *move = confirm.addButton(tr("Move store"), QMessageBox::AcceptRole);
            confirm.setDefaultButton(QMessageBox::Cancel); confirm.setEscapeButton(QMessageBox::Cancel);
            confirm.exec();
            return confirm.clickedButton() == move;
        };
        render();
    }
    // The folder dialog and the move's confirmation; tests replace them.
    std::function<QString(const QString &)> pickFolder;
    std::function<bool(const QString &, const QString &, qint64)> confirmMove;

    void setBackend(IronRules::Transport send) { m_send = std::move(send); reload(); }
    void setProject(const QString &project)
    {
        if (project == m_project) return;
        m_project = project; m_notice->clear();
        reload();
    }
    QJsonObject view() const { return m_view; }
    bool editable() const { return !m_loading && !m_failed && !m_view.isEmpty() && isEnabled(); }

    void reload(const QString &keep = {})
    {
        const auto generation = ++m_generation;
        m_view = {}; m_failed = false; m_error->clear();
        if (!m_send || m_project.isEmpty()) { m_loading = false; render(); return; }
        m_loading = true; render();
        m_send({}, "get", {{"project", m_project}}, [guard = QPointer<ProjectMemoryPanel>(this), generation, keep](const QJsonObject &value, const QString &failure) {
            if (!guard || guard->m_generation != generation) return;
            guard->m_loading = false;
            const auto error = value.value("unsupported").toBool() ? tr("Update this computer's hgs to set project memory.") : failure;
            guard->m_failed = !error.isEmpty();
            guard->m_error->setText(error.isEmpty() ? keep : error);
            if (error.isEmpty()) guard->m_view = ProjectMemory::viewOf(value);
            guard->render();
        });
    }
    // Browse: an empty store takes the folder; one with records moves.
    void browse()
    {
        if (!editable() || !pickFolder) return;
        const auto before = m_view;
        const auto picked = pickFolder(before.value("store").toString());
        if (picked.isEmpty() || picked == before.value("store").toString() || m_view != before) return;
        if (records() > 0) moveTo(picked); else write("set", {{"store", picked}});
    }
    void moveTo(const QString &to)
    {
        if (!editable() || to.trim().isEmpty() || to.trim() == m_view.value("store").toString()) return;
        const auto before = m_view;
        if (!confirmMove || !confirmMove(before.value("store").toString(), to.trim(), records())) return;
        if (m_view != before) return;
        write("move", {{"to", to.trim()}});
    }
    void useDefault()
    {
        if (!editable() || m_view.value("default").toBool()) return;
        if (records() > 0) moveTo(m_view.value("default_store").toString()); else write("set", {{"store", QString()}});
    }

private:
    qint64 records() const { return m_view.value("status").toObject().value("records").toInteger(); }
    void write(const QString &op, QJsonObject input)
    {
        if (!editable()) return;
        input.insert("project", m_project);
        input.insert("expected_revision", m_view.value("revision").toInteger());
        input.insert("request_id", QUuid::createUuid().toString(QUuid::WithoutBraces));
        m_loading = true; m_error->clear(); m_notice->clear(); render();
        const auto generation = m_generation;
        m_send({}, op, input, [guard = QPointer<ProjectMemoryPanel>(this), generation](const QJsonObject &value, const QString &failure) {
            if (!guard || guard->m_generation != generation) return;
            guard->m_loading = false;
            if (value.value("unsupported").toBool()) { guard->m_failed = true; guard->m_error->setText(tr("Update this computer's hgs to set project memory.")); guard->render(); return; }
            // A refused or lost write may have run: read again, keep the reason.
            if (!failure.isEmpty() || ProjectMemory::viewOf(value).isEmpty()) { guard->reload(failure.isEmpty() ? tr("Invalid project memory response.") : failure); return; }
            guard->m_view = ProjectMemory::viewOf(value);
            if (value.value("status").toString() == QStringLiteral("moved"))
                guard->m_notice->setText(value.value("old_kept").toBool()
                    ? tr("Moved %1 files (%2) and checked them. The old store is kept at %3.").arg(value.value("files").toInteger()).arg(ProjectMemory::sizeText(value.value("bytes").toInteger()), value.value("from").toString())
                    : tr("The store is now at %1.").arg(value.value("to").toString()));
            guard->render();
        });
    }
    void render()
    {
        setVisible(!m_project.isEmpty());
        const bool has = !m_view.isEmpty();
        m_switch->setChecked(m_view.value("enabled").toBool());
        m_switch->setEnabled(editable());
        m_store->setText(has ? m_view.value("store").toString() + (m_view.value("default").toBool() ? QStringLiteral(" ") + tr("(default)") : QString()) : QString());
        m_browse->setText(records() > 0 ? tr("Move…") : tr("Browse…"));
        m_browse->setToolTip(records() > 0 ? tr("The store holds records: they move with it, checked, and the old store is kept.") : tr("Choose the folder of the store."));
        m_browse->setEnabled(editable());
        m_default->setVisible(has && !m_view.value("default").toBool());
        m_default->setEnabled(editable());
        QString status = has ? ProjectMemory::statusText(m_view) : QString();
        if (has && !m_view.value("enabled").toBool() && m_view.value("status").toObject().value("exists").toBool())
            status += '\n' + tr("Memory is off; the store is kept as it is.");
        const auto old = m_view.value("moved_from").toArray();
        if (!old.isEmpty()) {
            status += '\n' + tr("Earlier stores, kept:");
            for (const auto &folder : old) status += '\n' + folder.toString();
        }
        m_status->setText(status);
    }
    IronRules::Transport m_send;
    QString m_project;
    QJsonObject m_view;
    quint64 m_generation = 0;
    bool m_loading = false, m_failed = false;
    QCheckBox *m_switch;
    QLabel *m_store, *m_status, *m_notice, *m_error;
    QPushButton *m_browse, *m_default;
};
