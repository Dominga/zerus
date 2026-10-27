#pragma once

#include "JsonParse.h"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QObject>
#include <QStringList>

namespace QuestionReply {
inline QJsonArray parse(QString text)
{
    text = text.trimmed();
    if (text.startsWith("# Context from my IDE setup:\n")) {
        const QString marker = "\n## My request for Codex:\n";
        const auto request = text.lastIndexOf(marker);
        if (request < 0) return {};
        text = text.mid(request + marker.size()).trimmed();
    }
    const QString start = "<send_user_message_question_reply>";
    const QString end = "</send_user_message_question_reply>";
    if (!text.startsWith(start) || !text.endsWith(end)) return {};
    QString error;
    const auto document = JsonParse::local(text.mid(start.size(), text.size() - start.size() - end.size()).toUtf8(), &error);
    if (!error.isEmpty()) return {};
    const auto replies = document.isObject() ? QJsonArray{document.object()} : document.array();
    for (const auto &value : replies) {
        if (!value.isObject()) return {};
        const auto reply = value.toObject();
        if (!reply.value("questionItemId").isString() || reply.value("questionItemId").toString().isEmpty()
            || !reply.value("question").isString() || reply.value("question").toString().trimmed().isEmpty()
            || !reply.value("answer").isString()) return {};
    }
    return replies;
}

inline QString preview(const QJsonArray &replies)
{
    QStringList answers;
    for (const auto &value : replies) {
        const auto answer = value.toObject().value("answer").toString();
        answers << (answer.isEmpty() ? QObject::tr("(empty answer)") : answer);
    }
    return (replies.size() == 1 ? QObject::tr("Your answer: %1") : QObject::tr("Your answers: %1"))
        .arg(answers.join(QStringLiteral(" / ")));
}
}
