#pragma once

#include "HgsClient.h"
#include "SessionPresentation.h"

#include <QJsonObject>
#include <QString>

// Rules the Sessions list and the window share for a listed session: its
// subagent roster, which children show, attention and unsent drafts. They read
// only their arguments, so the row preparation runs them off the GUI thread on
// copies (SessionsWindow::prepareRows), and the window on its own state.
namespace SessionRows {

// A listed session: host is empty for this computer, machine its name; key is
// host, name and, for an archived row, its archive; identity the organization's.
struct Entry { QString host, machine, key, identity; SessionInfo session; bool online = false; };

// The selected session's latest inspection and its open subagent.
struct Selection { QString key; QJsonObject details; QString subagentId, subagentConversation; };

inline QJsonObject roster(const Entry &entry, const Selection &selection)
{
    if (entry.session.subagentSource == "hook_profiles") return {};
    if (entry.key == selection.key && selection.details.contains("subagents")
        && (entry.session.conversationId.isEmpty() || entry.session.conversationId == selection.details.value("conversation_id").toString())
        && selection.details.value("last_event_at").toDouble() >= entry.session.lastEventAt)
        return selection.details.value("subagents").toObject();
    return entry.session.subagents;
}

// The conversation a child's draft belongs to.
inline QString draftConversation(const Entry &entry, const Selection &selection)
{
    return !entry.session.conversationId.isEmpty() ? entry.session.conversationId
        : entry.key == selection.key ? selection.details.value("conversation_id").toString() : QString();
}

inline QString childDraftKey(const Entry &entry, const QString &conversation, const QString &id)
{
    return entry.machine + '\n' + entry.session.name + '\n' + conversation + '\n' + id;
}

inline bool childAction(const QJsonObject &child)
{
    const auto state = SessionPresentation::childState(child);
    // Routine results and failures belong to the parent agent. Escalate only
    // explicit requests for user intervention.
    return state == "approval" || state == "input" || state == "attention";
}

inline QString childName(const QJsonObject &child, const QString &id)
{
    auto name = child.value("label").toString(child.value("name").toString());
    return name.isEmpty() ? id : name;
}

// hasChildDraft(key): whether the subagent composer holds an unsent draft.
template <typename ChildDraft>
bool showChild(const Entry &entry, const QString &id, const QJsonObject &child, bool countsComplete, bool includeCompleted,
               const Selection &selection, ChildDraft &&hasChildDraft)
{
    const auto conversation = draftConversation(entry, selection);
    return includeCompleted
        || (entry.key == selection.key && id == selection.subagentId && !id.isEmpty()
            && selection.subagentConversation == conversation)
        || (!child.value("group").toBool() && hasChildDraft(childDraftKey(entry, conversation, id)))
        || !SessionPresentation::inactiveChild(child, countsComplete);
}

inline bool childNeedsAction(const Entry &entry, const Selection &selection)
{
    if (!SessionPresentation::currentActivity(entry.session, entry.online)) return false;
    const auto children = roster(entry, selection);
    for (const auto &child : children) if (childAction(child.toObject())) return true;
    return false;
}

inline bool needsAttention(const Entry &entry, const Selection &selection)
{
    return SessionPresentation::needsAttention(entry.session, entry.online)
        || (entry.online && !entry.session.attentionAcknowledged && childNeedsAction(entry, selection));
}

// hasDraft(key) and hasChildDraft(key): the message and subagent composers.
template <typename Draft, typename ChildDraft>
bool hasDraft(const Entry &entry, const Selection &selection, Draft &&hasMessageDraft, ChildDraft &&hasChildDraft)
{
    if (entry.session.state == "archived") return false;
    if (hasMessageDraft(entry.key)) return true;
    const auto children = roster(entry, selection);
    const auto conversation = draftConversation(entry, selection);
    for (auto it = children.begin(); it != children.end(); ++it)
        if (!it.value().toObject().value("group").toBool() && hasChildDraft(childDraftKey(entry, conversation, it.key()))) return true;
    return false;
}

}
