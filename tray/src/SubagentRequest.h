#pragma once
#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonObject>
#include <QList>
#include <QRegularExpression>
#include <QString>

// WhiteWh fork, U19 (#520): a "Start N subagents" approval card. hgs marks the
// card (subagent_confirmation, subagent_calls) and lists its calls in
// `subagents` ({id, type, model, effort when known, description}); an older
// hgs sends only the question text "Start N subagents: type (model M, effort
// E): task; ...", which is read back here. The card is answered as any
// permission card: Allow all is its own one-time Yes, Deny its No, and
// "Allow all, don't ask again" its broader Yes that stops asking.
namespace SubagentRequest {

struct Call { QString id, type, model, effort, description; };

inline bool is(const QJsonObject &card) { return card.value("subagent_confirmation").toBool(); }

// "type (model M[, effort E])[: description]" items after "Start N subagent(s): ".
inline QList<Call> parse(const QString &question)
{
    QList<Call> calls;
    QString text = question.section(QStringLiteral("\n\n"), 0, 0).trimmed();
    static const QRegularExpression head(QStringLiteral("^Start \\d+ subagents?: "));
    const auto match = head.match(text);
    if (!match.hasMatch()) return calls;
    text = text.mid(match.capturedLength());
    // A new item starts after "; " where a "type (model " follows.
    static const QRegularExpression split(QStringLiteral(";\\s+(?=[^;()]+ \\(model )"));
    static const QRegularExpression item(QStringLiteral("^(.+?) \\(model ([^,)]+)(?:, effort ([^)]+))?\\)(?:: (.*))?$"),
                                         QRegularExpression::DotMatchesEverythingOption);
    for (const auto &part : text.split(split, Qt::SkipEmptyParts)) {
        const auto m = item.match(part.trimmed());
        if (!m.hasMatch()) { calls.append({{}, part.trimmed(), {}, {}, {}}); continue; }
        calls.append({{}, m.captured(1).trimmed(), m.captured(2).trimmed(), m.captured(3).trimmed(), m.captured(4).trimmed()});
    }
    return calls;
}

inline QList<Call> fromList(const QJsonArray &list)
{
    QList<Call> calls;
    for (const auto &value : list) {
        const auto o = value.toObject();
        calls.append({o.value("id").toString(), o.value("type").toString(), o.value("model").toString(),
                      o.value("effort").toString(), o.value("description").toString()});
    }
    return calls;
}

// The rows of a card or a member's permission record: its list, else its text.
inline QList<Call> calls(const QJsonObject &card, const QString &fallbackText)
{
    if (card.value("subagents").isArray() && !card.value("subagents").toArray().isEmpty()) return fromList(card.value("subagents").toArray());
    return parse(fallbackText);
}

// How many subagents the card starts: its rows, else its call ids.
inline int count(const QJsonObject &card, const QList<Call> &rows)
{
    return qMax(int(rows.size()), int(card.value("subagent_calls").toArray().size()));
}

inline QString title(int n)
{
    return n == 1 ? QCoreApplication::translate("SubagentRequest", "Start 1 subagent")
                  : QCoreApplication::translate("SubagentRequest", "Start %1 subagents").arg(n);
}

// The card's own option ids: Yes allows, No denies (claude_approval's
// one-time choices); none when the card lacks them.
inline QString optionId(const QJsonObject &question, const QString &label)
{
    for (const auto &value : question.value("options").toArray()) {
        const auto option = value.toObject();
        if (option.value("label").toString().compare(label, Qt::CaseInsensitive) == 0) return option.value("id").toString();
    }
    return {};
}

// The card's option that allows every subagent and stops asking: hgs's own
// "Yes, and don't ask again for subagents" on a hook card, Claude's "Yes, and
// don't ask again ..." on a screen card. A broader Yes only; none without one.
inline QString alwaysOptionId(const QJsonObject &question)
{
    for (const auto &value : question.value("options").toArray()) {
        const auto option = value.toObject();
        QString label = option.value("label").toString().simplified();
        label.replace(QChar(0x2019), QLatin1Char('\''));
        if (option.value("scope").toString() == QLatin1String("broader") && label.startsWith(QLatin1String("Yes"))
            && label.contains(QLatin1String("don't ask again")))
            return option.value("id").toString();
    }
    return {};
}

}
