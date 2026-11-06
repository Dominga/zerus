#include "DiskSettings.h"
#include "TeamController.h"
#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QMessageBox>
#include <QPushButton>
#include <QSpinBox>
#include <QVBoxLayout>

namespace {
QLabel *line(const QString &text, const char *name, bool wrap = true)
{
    auto *label = new QLabel(text);
    label->setObjectName(name);
    label->setWordWrap(wrap);
    label->setTextInteractionFlags(Qt::TextSelectableByMouse);
    return label;
}
}

DiskSettings::DiskSettings(TeamController *teams, QWidget *parent) : QWidget(parent), m_teams(teams)
{
    setObjectName("diskSettings");
    auto *outer = new QVBoxLayout(this);
    outer->setContentsMargins(24, 20, 24, 20);
    outer->setSpacing(10);
    auto *title = new QLabel(tr("Build folders")); title->setObjectName("settingsTitle");
    outer->addWidget(title);
    outer->addWidget(line(tr("Zerus removes only the build folders it created or was told to use, never a folder of yours, never sources, and never a folder "
                             "a build or test is using; a removal that may have half happened waits for your review."), "diskSettingsNote"));
    auto *form = new QFormLayout;
    m_enabled = new QCheckBox(tr("Clean up automatically on this computer")); m_enabled->setObjectName("diskPolicyEnabled");
    m_days = new QSpinBox; m_days->setObjectName("diskPolicyDays"); m_days->setRange(0, 365);
    m_freeGb = new QSpinBox; m_freeGb->setObjectName("diskPolicyFreeGb"); m_freeGb->setRange(0, 10000);
    m_freePercent = new QSpinBox; m_freePercent->setObjectName("diskPolicyFreePercent"); m_freePercent->setRange(0, 90);
    form->addRow(m_enabled);
    form->addRow(tr("Remove a folder unused for (days, 0: never)"), m_days);
    form->addRow(tr("Keep this much free on its drive (GB)"), m_freeGb);
    form->addRow(tr("or this share of the drive (%)"), m_freePercent);
    outer->addLayout(form);
    m_save = new QPushButton(tr("Save policy")); m_save->setObjectName("diskPolicySave");
    auto *policyRow = new QHBoxLayout; policyRow->addWidget(m_save); policyRow->addStretch();
    outer->addLayout(policyRow);
    auto *actions = new QHBoxLayout;
    m_scope = new QComboBox; m_scope->setObjectName("diskCleanScope");
    m_scope->addItem(tr("What the policy removes now"), QStringLiteral("policy"));
    m_scope->addItem(tr("Every folder of Zerus's"), QStringLiteral("all"));
    m_dry = new QPushButton(tr("Dry run")); m_dry->setObjectName("diskDryRun");
    m_clean = new QPushButton(tr("Clean now…")); m_clean->setObjectName("diskCleanNow");
    m_measure = new QPushButton(tr("Measure again")); m_measure->setObjectName("diskMeasure");
    actions->addWidget(m_scope); actions->addWidget(m_dry); actions->addWidget(m_clean); actions->addWidget(m_measure); actions->addStretch();
    outer->addLayout(actions);
    m_notice = line(QString(), "diskNotice"); m_notice->hide();
    m_summary = line(QString(), "diskSummary"); m_summary->hide();
    outer->addWidget(m_notice);
    outer->addWidget(m_summary);
    m_rows = new QVBoxLayout; m_rows->setSpacing(4);
    outer->addLayout(m_rows);
    outer->addStretch();
    connect(m_save, &QPushButton::clicked, this, [this] { savePolicy(); });
    connect(m_dry, &QPushButton::clicked, this, [this] { clean(true); });
    connect(m_clean, &QPushButton::clicked, this, [this] { clean(false); });
    connect(m_measure, &QPushButton::clicked, this, [this] { refresh(true); });
    refresh();
}

QString DiskSettings::size(qint64 bytes, bool known)
{
    if (!known) return tr("not measured yet");
    const double gb = double(bytes) / (1024.0 * 1024.0 * 1024.0);
    if (gb >= 0.1) return tr("%1 GB").arg(gb, 0, 'f', 1);
    return tr("%1 MB").arg(double(bytes) / (1024.0 * 1024.0), 0, 'f', 0);
}

void DiskSettings::send(const QJsonObject &input, const std::function<void(const QJsonObject &)> &then)
{
    if (!m_teams || m_busy) return;
    m_busy = true;
    for (auto *button : {m_dry, m_clean, m_measure, m_save}) button->setEnabled(false);
    QJsonObject request = input;
    request.insert("by", "human");
    m_teams->call(QStringLiteral("disk"), request, [this, then](bool ok, const QJsonObject &result, const QString &error) {
        m_busy = false;
        for (auto *button : {m_dry, m_clean, m_measure, m_save}) button->setEnabled(true);
        if (!ok) { m_notice->setText(error); m_notice->show(); return; }
        m_notice->hide();
        then(result);
    }, false, 120000);
}

void DiskSettings::refresh(bool measure)
{
    QJsonObject input{{"action", "list"}};
    if (measure) input.insert("refresh", true);
    send(input, [this](const QJsonObject &result) {
        m_view = result;
        const auto policy = result["policy"].toObject();
        m_enabled->setChecked(policy["enabled"].toBool(true));
        m_days->setValue(policy["unused_days"].toInt(7));
        m_freeGb->setValue(policy["free_gb"].toInt(50));
        m_freePercent->setValue(policy["free_percent"].toInt(10));
        render();
    });
}

void DiskSettings::savePolicy()
{
    const QJsonObject policy{{"enabled", m_enabled->isChecked()}, {"unused_days", m_days->value()},
                             {"free_gb", m_freeGb->value()}, {"free_percent", m_freePercent->value()}};
    send(QJsonObject{{"action", "policy"}, {"policy", policy}}, [this](const QJsonObject &) { refresh(); });
}

void DiskSettings::clean(bool dry)
{
    const QString scope = m_scope->currentData().toString();
    if (!dry) {
        const QString text = scope == QLatin1String("all")
            ? tr("Remove every build folder Zerus holds on this computer? Only Zerus's own build output goes; a folder in use, or one that is not Zerus's, is kept. Builds make them again.")
            : tr("Remove the build folders the cleanup policy marks now? Only Zerus's own build output goes; a folder in use, or one that is not Zerus's, is kept. Builds make them again.");
        bool yes = false;
        if (confirm) yes = confirm(QStringLiteral("diskCleanConfirm"), tr("Remove build folders?"), text, tr("Remove"));
        else {
            QMessageBox box(QMessageBox::Warning, tr("Remove build folders?"), text, QMessageBox::NoButton, this);
            auto *remove = box.addButton(tr("Remove"), QMessageBox::DestructiveRole);
            auto *cancel = box.addButton(QMessageBox::Cancel);
            box.setDefaultButton(cancel);
            box.exec();
            yes = box.clickedButton() == remove;
        }
        if (!yes) return;
    }
    send(QJsonObject{{"action", "clean"}, {"scope", scope}, {"dry_run", dry}}, [this, dry](const QJsonObject &result) {
        m_decisions = result["decisions"].toArray();
        m_dryShown = dry;
        if (!dry) refresh(); else render();
    });
}

void DiskSettings::forget(const QString &path)
{
    send(QJsonObject{{"action", "forget"}, {"path", path}}, [this](const QJsonObject &) { refresh(); });
}

void DiskSettings::render()
{
    while (auto *item = m_rows->takeAt(0)) {
        if (auto *widget = item->widget()) widget->deleteLater();
        delete item;
    }
    // The last dry run or clean.
    if (!m_decisions.isEmpty() || !m_dryShown) {
        qint64 bytes = 0; int removed = 0;
        for (const auto &value : m_decisions) {
            const auto decision = value.toObject();
            if (decision["action"] == QLatin1String("remove")) { ++removed; bytes += qint64(decision["bytes"].toDouble()); }
        }
        const int kept = int(m_decisions.size()) - removed;
        QString text = m_decisions.isEmpty() ? tr("Nothing to remove now")
            : (m_dryShown ? tr("Would free %1 in %2 folders").arg(size(bytes)).arg(removed) : tr("Freed %1 in %2 folders").arg(size(bytes)).arg(removed));
        if (kept > 0) text = tr("%1; %2 kept").arg(text).arg(kept);
        m_summary->setText(text); m_summary->show();
        for (const auto &value : m_decisions) {
            const auto decision = value.toObject();
            const QString why = decision["action"] == QLatin1String("remove")
                ? tr("%1; %2").arg(decision["reason"].toString(), size(qint64(decision["bytes"].toDouble())))
                : decision["detail"].toString();
            auto *row = line(QStringLiteral("%1   %2").arg(decision["path"].toString(), why), "diskDecision");
            row->setProperty("action", decision["action"].toString());
            m_rows->addWidget(row);
        }
    } else {
        m_summary->hide();
    }
    for (const auto &value : m_view["drives"].toArray()) {
        const auto drive = value.toObject();
        QString text = tr("%1 %2 free of %3, keeps %4").arg(drive["drive"].toString(), size(qint64(drive["free"].toDouble())),
                                                             size(qint64(drive["total"].toDouble())), size(qint64(drive["threshold"].toDouble())));
        if (drive["low"].toBool()) text = tr("%1; low on space").arg(text);
        auto *row = line(text, "diskDrive");
        row->setProperty("low", drive["low"].toBool());
        m_rows->addWidget(row);
    }
    m_rows->addWidget(line(tr("Zerus holds %1 in build folders").arg(size(qint64(m_view["held"].toDouble()))), "diskHeld"));
    const auto teams = m_view["teams"].toArray();
    if (teams.isEmpty()) m_rows->addWidget(line(tr("Zerus has recorded no build folder on this computer yet"), "diskEmpty"));
    for (const auto &value : teams) {
        const auto team = value.toObject();
        const QString name = team["team_name"].toString().isEmpty() ? tr("Sessions") : team["team_name"].toString();
        auto *head = line(tr("%1   %2").arg(name, size(qint64(team["bytes"].toDouble()))), "diskTeam");
        head->setProperty("team", team["team"].toString());
        m_rows->addWidget(head);
        for (const auto &item : team["folders"].toArray()) {
            const auto folder = item.toObject();
            QString state;
            if (folder["state"] == QLatin1String("review")) state = tr("Waits for your review: %1").arg(folder["note"].toString());
            else if (!folder["owned"].toBool()) state = tr("Kept: not Zerus's (%1)").arg(folder["owned_reason"].toString());
            else if (!folder["exists"].toBool(true)) state = tr("Gone");
            else if (folder["due"].isString()) state = tr("The policy removes it: %1").arg(folder["due"].toString());
            else state = tr("Kept");
            auto *row = new QWidget; row->setObjectName("diskFolder"); row->setProperty("path", folder["path"].toString());
            auto *h = new QHBoxLayout(row); h->setContentsMargins(12, 0, 0, 0);
            h->addWidget(line(folder["path"].toString(), "diskFolderPath", false), 1);
            h->addWidget(line(size(qint64(folder["size"].toDouble()), folder.contains("size")), "diskFolderSize", false));
            h->addWidget(line(state, "diskFolderState"), 1);
            if (folder["state"] == QLatin1String("review")) {
                auto *dealt = new QPushButton(tr("I dealt with it")); dealt->setObjectName("diskForget");
                const QString path = folder["path"].toString();
                connect(dealt, &QPushButton::clicked, this, [this, path] { forget(path); });
                h->addWidget(dealt);
            }
            m_rows->addWidget(row);
        }
    }
}
