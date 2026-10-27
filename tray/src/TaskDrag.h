#pragma once
#include "JsonParse.h"
#include "TaskBoard.h"
#include <QJsonDocument>
#include <QJsonObject>
#include <QMetaType>
#include <QMimeData>
#include <optional>

// A task dragged from either board surface. Every writable stage can be
// reordered; stage changes and session routing have their own guards.
struct TaskDrag {
    QString projectId, uuid, id, stage;
    // Its pinned session; an empty name is Any session. targetKnown is false for a
    // computer this one has not verified.
    QString targetHost, targetSession;
    bool targetKnown = true;

    static QString mimeType() { return QStringLiteral("application/x-zerus-task"); }
    // Every stage supports board ordering; future-schema documents stay read-only.
    static bool draggable(const TaskItem &task) {
        return task.writable && TaskBoard::stages().contains(task.stage);
    }
    static TaskDrag of(const QString &projectId, const TaskItem &task) {
        return {projectId, task.uuid, task.id, task.stage, task.targetHost, task.targetSession, task.targetKnown};
    }
    QMimeData *mimeData() const {
        auto *mime = new QMimeData;
        const QJsonObject data{{"project", projectId}, {"uuid", uuid}, {"id", id}, {"stage", stage},
                               {"host", targetHost}, {"session", targetSession}, {"known", targetKnown}};
        mime->setData(mimeType(), QJsonDocument(data).toJson(QJsonDocument::Compact));
        return mime;
    }
    static std::optional<TaskDrag> from(const QMimeData *mime) {
        if (!mime || !mime->hasFormat(mimeType())) return std::nullopt;
        const auto data = JsonParse::localObject(mime->data(mimeType()));
        TaskDrag drag{data.value("project").toString(), data.value("uuid").toString(), data.value("id").toString(),
                      data.value("stage").toString(), data.value("host").toString(), data.value("session").toString(),
                      data.value("known").toBool(true)};
        if (drag.projectId.isEmpty() || drag.uuid.isEmpty()) return std::nullopt;
        return drag;
    }
};
Q_DECLARE_METATYPE(TaskDrag)
