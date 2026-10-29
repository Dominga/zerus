#pragma once
#include <QJsonObject>
#include <QString>
#include <QStringList>
#include <QUrl>
#include <QRegularExpression>

// Presentation only. Model names, login type and an MCP connection do not
// establish whether an external conversation belongs to Chat or Work.
namespace ConversationSource {
inline QString kind(const QString &agent,const QJsonObject &context) {
    if(agent!="external" || !context["schema_version"].isDouble() || context["schema_version"].toDouble()!=1
       || context["evidence"]!="explicit_registration") return {};
    const auto value=context["kind"].toString();
    return QStringList{"chatgpt_chat","chatgpt_work","codex_chat"}.contains(value)?value:QString();
}
inline QString label(const QString &agent,const QJsonObject &context) {
    const auto value=kind(agent,context);
    if(value=="chatgpt_chat")return QStringLiteral("ChatGPT Chat");
    if(value=="chatgpt_work")return QStringLiteral("ChatGPT Work");
    if(value=="codex_chat")return QStringLiteral("Codex Chat");
    return agent=="external"?QStringLiteral("External - unknown source"):QString();
}
inline QString badge(const QString &agent,const QJsonObject &context) {
    const auto value=kind(agent,context);return value.isEmpty()?agent:value;
}
inline bool cooperative(const QJsonObject &raw) {
    return raw["agent"]=="external" && raw["backend"]=="external-workstream"
        && raw["external_protocol"]=="cooperative-inbox-v1" && raw["send_delivery"]=="cooperative_inbox"
        && raw["external_cloud_control"].isBool() && !raw["external_cloud_control"].toBool()
        && !kind("external",raw["client_context"].toObject()).isEmpty()
        && raw["tracked"].isBool() && raw["tracked"].toBool()
        && raw["send_available"].isBool() && raw["send_available"].toBool()
        && raw["send_supported"].isBool() && raw["send_supported"].toBool()
        && !raw["run_id"].toString().isEmpty() && !raw["conversation_id"].toString().isEmpty();
}
inline QUrl chatUrl(const QString &value) {
    static const QRegularExpression exact("^https://chatgpt[.]com/c/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$");
    return exact.match(value).hasMatch()?QUrl(value):QUrl();
}
inline QString queueWarning() {return QStringLiteral("External inbox only: this does not send to or wake ChatGPT. The agent must poll and acknowledge the instruction through Desktop Commander.");}
}
