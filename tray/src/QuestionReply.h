#pragma once

#include "JsonParse.h"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QObject>
#include <QStringList>

// The user's own answers to an agent's questions, which a session's prompt or
// activity detail may carry as JSON. They are parsed off the GUI thread: with
// the row (sessionFromJson) or, for a row or an inspection that a GUI-thread
// conversion reads, by the prepare step that parsed it (annotate()). The GUI
// thread reads the finished previews from SessionInfo.
namespace QuestionReply {
inline const QString &start() { static const QString value = QStringLiteral("<send_user_message_question_reply>"); return value; }
inline const QString &end() { static const QString value = QStringLiteral("</send_user_message_question_reply>"); return value; }

// The answers in text. It parses JSON: off the GUI thread only.
inline QJsonArray parse(QString text, const char *file = __builtin_FILE(), int line = __builtin_LINE())
{
    text = text.trimmed();
    if (text.startsWith("# Context from my IDE setup:\n")) {
        const QString marker = "\n## My request for Codex:\n";
        const auto request = text.lastIndexOf(marker);
        if (request < 0) return {};
        text = text.mid(request + marker.size()).trimmed();
    }
    if (!text.startsWith(start()) || !text.endsWith(end())) return {};
    QString error;
    const auto document = JsonParse::parse(text.mid(start().size(), text.size() - start().size() - end().size()).toUtf8(), &error, file, line);
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

// "Your answer: …" for the answers in text, or empty when it holds none.
// Off the GUI thread only.
inline QString previewOf(const QString &text, const char *file = __builtin_FILE(), int line = __builtin_LINE())
{
    const auto replies = parse(text, file, line);
    return replies.isEmpty() ? QString() : preview(replies);
}

// Where annotate() records the preview of field, a key hgs never sends.
inline QString previewKey(const QString &field) { return QStringLiteral("zerus_answers_preview:") + field; }

// A prepare step that parsed object, a session row or an inspection, records
// the previews of its prompt and activity_detail for a GUI-thread conversion.
inline void annotate(QJsonObject &object)
{
    for (const QString field : {QStringLiteral("prompt"), QStringLiteral("activity_detail")}) {
        const auto text = object.value(field).toString();
        if (text.contains(start())) object.insert(previewKey(field), previewOf(text));
        else object.remove(previewKey(field));
    }
}
inline void annotateRows(QJsonArray &rows)
{
    for (qsizetype i = 0; i < rows.size(); ++i) {
        if (!rows.at(i).isObject()) continue;
        auto row = rows.at(i).toObject(); annotate(row); rows.replace(i, row);
    }
}

// The preview of object's field: the one annotate() recorded or, off the GUI
// thread, parsed now. A GUI-thread read of answers nobody prepared is reported
// like a parse (JsonParse.h) and reads as none.
inline QString prepared(const QJsonObject &object, const QString &field,
                        const char *file = __builtin_FILE(), int line = __builtin_LINE())
{
    const auto recorded = object.constFind(previewKey(field));
    if (recorded != object.constEnd()) return recorded->toString();
    const auto text = object.value(field).toString();
    if (!text.contains(start())) return {};
    if (JsonParse::onGuiThread()) { JsonParse::reportUnprepared(file, line); return {}; }
    return previewOf(text, file, line);
}
}
