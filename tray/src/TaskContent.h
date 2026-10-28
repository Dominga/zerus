#pragma once
#include "HgsClient.h"
#include <QJsonArray>
#include <QJsonDocument>

namespace TaskContent {
inline QJsonArray encode(const QList<MessageAttachment> &attachments) {
    QJsonArray out;
    for (const auto &file : attachments) out << QJsonObject{{"name", file.name}, {"mime", file.mime},
        {"reference", file.reference}, {"data_base64", QString::fromLatin1(file.data.toBase64())}};
    return out;
}
inline QList<MessageAttachment> decode(const QJsonArray &files) {
    QList<MessageAttachment> out;
    if (files.size() > HgsClient::MaximumAttachments) return out;
    qsizetype total = 0;
    for (const auto &value : files) {
        const auto file = value.toObject();
        const auto encoded = file.value("data_base64").toString().toLatin1();
        if (encoded.size() > (HgsClient::MaximumAttachmentBytes + 2) / 3 * 4) return {};
        const auto bytes = QByteArray::fromBase64Encoding(encoded, QByteArray::AbortOnBase64DecodingErrors);
        total += bytes.decoded.size();
        if (!bytes || bytes.decoded.isEmpty() || bytes.decoded.size() > HgsClient::MaximumAttachmentBytes || total > HgsClient::MaximumAttachmentsBytes) return {};
        out << MessageAttachment{file.value("name").toString(), file.value("mime").toString(), bytes.decoded, file.value("reference").toString()};
    }
    return out;
}
inline QString payload(const QString &text, const QJsonArray &files) {
    return QString::fromUtf8(QJsonDocument(QJsonObject{{"text", text}, {"attachments", files}}).toJson(QJsonDocument::Compact));
}
}
