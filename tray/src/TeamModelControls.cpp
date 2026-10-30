#include "TeamModelControls.h"
#include "TeamModel.h"
#include <QComboBox>
#include <QCoreApplication>
#include <QJsonArray>
#include <QSignalBlocker>

namespace TeamModelControls {
QString key(const QString &agent, const QString &host, const QString &account)
{
    const QString provider = agent.isEmpty() ? QStringLiteral("claude") : agent;
    return host + QLatin1Char('\n') + (account.isEmpty() ? QStringLiteral("native-") + provider : account);
}

bool supportsCatalog(const QString &agent)
{
    return QStringList{QStringLiteral("claude"), QStringLiteral("codex"), QStringLiteral("kimi"), QStringLiteral("dsh")}.contains(agent);
}

void fillModels(QComboBox *box, const QJsonObject &catalog, const QString &agent, const QString &selected)
{
    const QSignalBlocker blocker(box);
    box->clear();
    const auto models = catalog.value("models").toArray();
    if (models.isEmpty()) box->addItems(Teams::knownModels(agent));
    for (const auto &value : models) {
        const QString id = value.toObject().value("id").toString();
        if (!id.isEmpty() && box->findText(id) < 0) box->addItem(id, id);
    }
    if (!selected.isEmpty() && box->findText(selected) < 0) box->addItem(selected, selected);
    box->setCurrentText(selected);
}

void fillEfforts(QComboBox *box, const QJsonObject &catalog, const QString &model, const QString &selected)
{
    const QSignalBlocker blocker(box);
    box->clear();
    box->addItem(QStringLiteral("—"), QString());
    QJsonObject chosen;
    for (const auto &value : catalog.value("models").toArray()) {
        const auto entry = value.toObject();
        if (entry.value("id").toString() == model) { chosen = entry; break; }
    }
    const auto options = chosen.value("effort_options").toArray();
    const auto labels = chosen.value("effort_labels").toArray();
    if (catalog.value("effort_supported").toBool()) {
        for (int i = 0; i < options.size(); ++i) {
            const QString effort = options.at(i).toString();
            if (!effort.isEmpty() && box->findData(effort) < 0)
                box->addItem(labels.at(i).toString(effort), effort);
        }
    }
    if (!model.isEmpty() && !selected.isEmpty() && box->findData(selected) < 0)
        box->addItem(QCoreApplication::translate("TeamModelControls", "%1 (unavailable)").arg(selected), selected);
    box->setCurrentIndex(qMax(0, box->findData(selected)));
    box->setEnabled(catalog.value("effort_supported").toBool() && !model.isEmpty() && (!options.isEmpty() || !selected.isEmpty()));
}
}
