#pragma once
#include <QJsonObject>
#include <QString>

class QComboBox;

namespace TeamModelControls {
QString key(const QString &agent, const QString &host, const QString &account);
bool supportsCatalog(const QString &agent);
void fillModels(QComboBox *box, const QJsonObject &catalog, const QString &agent, const QString &selected);
void fillEfforts(QComboBox *box, const QJsonObject &catalog, const QString &model, const QString &selected);
}
