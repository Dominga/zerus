#pragma once
#include "TaskBoard.h"
#include <QDataStream>
#include <QIODevice>
#include <QMetaType>
#include <QMimeData>
#include <optional>

// A task dragged from either board surface. Every writable stage can be
// reordered; stage changes and session routing have their own guards. The
// payload stays in this process, so it is a QDataStream that drops read
// without a JSON parse.
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
        QByteArray data;
        QDataStream stream(&data, QIODevice::WriteOnly); stream.setVersion(QDataStream::Qt_6_0);
        stream << projectId << uuid << id << stage << targetHost << targetSession << targetKnown;
        mime->setData(mimeType(), data);
        return mime;
    }
    static std::optional<TaskDrag> from(const QMimeData *mime) {
        if (!mime || !mime->hasFormat(mimeType())) return std::nullopt;
        QDataStream stream(mime->data(mimeType())); stream.setVersion(QDataStream::Qt_6_0);
        TaskDrag drag;
        stream >> drag.projectId >> drag.uuid >> drag.id >> drag.stage >> drag.targetHost >> drag.targetSession >> drag.targetKnown;
        if (stream.status() != QDataStream::Ok || !stream.atEnd() || drag.projectId.isEmpty() || drag.uuid.isEmpty()) return std::nullopt;
        return drag;
    }
};
Q_DECLARE_METATYPE(TaskDrag)
