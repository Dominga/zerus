#pragma once
#include "TeamModel.h"
#include <QHash>
#include <QJsonObject>
#include <QList>
#include <QSet>
#include <QWidget>
#include <functional>

class QVBoxLayout;

// WhiteWh fork: U57. The lead's open proposals as cards in the lead's
// conversation (the team's coordinator and the session window's Activity),
// drawn like the member permission card: what changes, who asks and why,
// Deny with an optional reason that reaches the lead, and Allow. An answer
// carries the proposal id and its content hash; a card whose proposal changed
// or was decided meanwhile says so and sends nothing.
class TeamProposalCards : public QWidget {
public:
    using Done = std::function<void(bool ok, const QString &message)>;
    explicit TeamProposalCards(QWidget *parent = nullptr);
    // The team's open proposals; none (or no team) hides the cards. The cards
    // are rebuilt only when what they show changes, so a typed reason stays.
    void setTeam(const Teams::Team *team);
    void setDark(bool dark);
    // #724: only this proposal's card (Waiting for you answers one item);
    // empty: every open proposal. Set before setTeam.
    void setOnly(const QString &proposal) { m_only = proposal; }
    // Sends decide-proposal; the input carries team, proposal, accept,
    // expected_hash and, when given, reason. Done's message on success is the
    // receipt's status ("accepted", "rejected").
    std::function<void(const QJsonObject &input, const Done &done)> decide;
    // The team as it is now, read again after a confirmation; empty: the last
    // team set.
    std::function<const Teams::Team *()> current;
    // Asks before an Allow that removes or replaces work (remove, swap,
    // cancel tasks, discard a phase); empty: a warning box of the card's own.
    std::function<bool(const QString &title, const QString &text, const QString &action)> confirm;
    // U59: a plan-mode switch also opens its own dialog (Details…); empty: no button.
    std::function<void(const QString &proposal)> details;
    int count() const { return int(m_views.size()); }
    QString teamId() const { return m_team; }
protected:
    bool event(QEvent *event) override;
    bool eventFilter(QObject *watched, QEvent *event) override;
private:
    // At most half the parent's height; the rest scrolls.
    void fit();
    struct View {
        QString id, hash, kind, by, reason, what;
        QStringList lines;
        // #740: a question's options and the one the lead recommends.
        QStringList options;
        QString recommended;
        double at = 0;
        bool operator==(const View &other) const
        {
            return id == other.id && hash == other.hash && kind == other.kind && by == other.by && reason == other.reason
                && what == other.what && lines == other.lines && options == other.options && recommended == other.recommended
                && at == other.at;
        }
    };
    static QList<View> viewsOf(const Teams::Team &team);
    void render();
    QWidget *cardFor(const View &view, bool gone);
    void answer(const View &view, bool allow);
    // #740: a question's card: an option or the owner's own words, Answer.
    void questionCard(const View &view, QVBoxLayout *v);
    void answerQuestion(const View &view);
    QHash<QString, QString> m_picked;   // by proposal: the chosen option, or empty for Other
    QSet<QString> m_ownWords;           // proposals answered with the owner's own words
    bool ask(const View &view);
    class QScrollArea *m_scroll;
    QWidget *m_inner;
    QVBoxLayout *m_layout;
    QString m_team, m_only;
    QList<View> m_views;
    // A card whose answer found its proposal changed or decided: the view it
    // was answered on and what the card says.
    QHash<QString, View> m_staleViews;
    QHash<QString, QString> m_notices;
    QHash<QString, QString> m_reasons;   // the typed Deny reasons, by proposal
    QSet<QString> m_sending;             // answered; until the store takes them
    bool m_dark = true, m_asking = false;
};
