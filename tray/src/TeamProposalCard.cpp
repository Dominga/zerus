#include "TeamProposalCard.h"
#include "TeamWidgets.h"
#include <QApplication>
#include <QDateTime>
#include <QLineEdit>
#include <QMessageBox>
#include <QButtonGroup>
#include <QPointer>
#include <QRadioButton>
#include <QScrollArea>
#include <QScrollBar>
#include <QTimer>
#include <QScopedValueRollback>
#include <algorithm>

using namespace TeamUi;

namespace {
QString kindLabel(const QString &kind)
{
    if (kind == "add-member") return TeamProposalCards::tr("Members");
    if (kind == "remove-member") return TeamProposalCards::tr("Remove");
    if (kind == "swap-member") return TeamProposalCards::tr("Swap");
    if (kind == "next-goal") return TeamProposalCards::tr("Next goal");
    if (kind == "switch-plan-mode") return TeamProposalCards::tr("Plan mode");
    if (kind == "add-to-phase") return TeamProposalCards::tr("Tasks");
    if (kind == "cancel-tasks") return TeamProposalCards::tr("Cancel");
    if (kind == "discard-phase") return TeamProposalCards::tr("Phase");
    if (kind == "question") return TeamProposalCards::tr("Question");
    if (kind == "switch-setting") return TeamProposalCards::tr("Setting");
    return kind;
}

QString askedAt(double at)
{
    if (at <= 0) return {};
    const auto when = QDateTime::fromSecsSinceEpoch(qint64(at));
    return when.date() == QDate::currentDate() ? when.toString(QStringLiteral("HH:mm")) : when.toString(QStringLiteral("d MMM HH:mm"));
}
}

TeamProposalCards::TeamProposalCards(QWidget *parent) : QWidget(parent)
{
    setObjectName("teamProposalCards");
    // The cards scroll once they would take more than half the column; each
    // keeps its whole height.
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(0);
    m_scroll = new QScrollArea; m_scroll->setObjectName("teamProposalCardsScroll");
    m_scroll->setWidgetResizable(true); m_scroll->setFrameShape(QFrame::NoFrame);
    m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_inner = new QWidget; m_inner->setObjectName("teamProposalCardsList");
    m_layout = new QVBoxLayout(m_inner); m_layout->setContentsMargins(10, 6, 10, 6); m_layout->setSpacing(6);
    m_scroll->setWidget(m_inner);
    // #754: once the cards are laid out, a refused one shows its answers.
    connect(m_scroll->verticalScrollBar(), &QScrollBar::rangeChanged, this, [this] { reveal(); });
    outer->addWidget(m_scroll);
    setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);
    if (parent) parent->installEventFilter(this);
    hide();
    setDark(true);
}

bool TeamProposalCards::event(QEvent *event)
{
    if (event->type() == QEvent::ParentChange && parentWidget()) parentWidget()->installEventFilter(this);
    if (event->type() == QEvent::Resize || event->type() == QEvent::Show) QTimer::singleShot(0, this, [this] { fit(); });
    return QWidget::event(event);
}

bool TeamProposalCards::eventFilter(QObject *watched, QEvent *event)
{
    if (watched == parentWidget() && event->type() == QEvent::Resize) QTimer::singleShot(0, this, [this] { fit(); });
    return QWidget::eventFilter(watched, event);
}

void TeamProposalCards::fit()
{
    if (m_layout->isEmpty()) return;
    const int width = qMax(200, m_scroll->viewport()->width());
    const int wanted = m_inner->hasHeightForWidth() ? m_inner->heightForWidth(width) : m_inner->sizeHint().height();
    const int room = parentWidget() ? parentWidget()->height() / 2 : 360;
    // #724/#740: one proposal's card in Waiting for you shows whole.
    const int height = m_only.isEmpty() ? qMax(80, qMin(wanted, qMax(160, room))) : wanted;
    if (height != this->height() || minimumHeight() != height) setFixedHeight(height);
}

QList<TeamProposalCards::View> TeamProposalCards::viewsOf(const Teams::Team &team)
{
    QList<View> views;
    for (const auto &proposal : team.proposals) {
        if (!proposal.open()) continue;
        View view;
        view.id = proposal.id; view.hash = proposal.hash; view.kind = proposal.kind;
        view.by = proposal.by.isEmpty() ? tr("The lead") : proposal.by;
        view.reason = proposal.reason; view.at = proposal.at;
        view.what = Teams::proposalText(team, proposal);
        view.lines = Teams::proposalChanges(team, proposal);
        if (proposal.kind == "question") {
            const auto change = proposal.change.toObject();
            for (const auto &option : change.value("options").toArray()) view.options << option.toString();
            view.recommended = change.value("recommended").toString();
        }
        views << view;
    }
    return views;
}

void TeamProposalCards::setTeam(const Teams::Team *team)
{
    const QString id = team ? team->id : QString();
    auto views = team ? viewsOf(*team) : QList<View>();
    if (!m_only.isEmpty()) views.erase(std::remove_if(views.begin(), views.end(), [this](const View &view) { return view.id != m_only; }), views.end());
    if (id != m_team) { m_staleViews.clear(); m_notices.clear(); m_reasons.clear(); m_sending.clear(); }
    QSet<QString> open;
    for (const auto &view : views) open << view.id;
    m_sending.intersect(open);   // the ones the store has taken are gone
    // A card that says something keeps saying it after its proposal is gone,
    // until the owner dismisses it; one shown again replaces it.
    for (const auto &view : std::as_const(m_views))
        if (!open.contains(view.id) && m_notices.contains(view.id)) m_staleViews.insert(view.id, view);
    for (auto it = m_staleViews.begin(); it != m_staleViews.end();) it = open.contains(it.key()) ? m_staleViews.erase(it) : std::next(it);
    for (auto it = m_reasons.begin(); it != m_reasons.end();) it = open.contains(it.key()) ? std::next(it) : m_reasons.erase(it);
    for (auto it = m_notices.begin(); it != m_notices.end();)
        it = open.contains(it.key()) || m_staleViews.contains(it.key()) ? std::next(it) : m_notices.erase(it);
    const bool same = id == m_team && views == m_views;
    m_team = id; m_views = views;
    if (same || m_asking) return;
    render();
}

void TeamProposalCards::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    setStyleSheet(teamSheet(c) + QStringLiteral("QWidget#teamProposalCards,QScrollArea#teamProposalCardsScroll,QWidget#teamProposalCardsList{background:transparent;}"));
    m_scroll->viewport()->setAutoFillBackground(false);
    render();
}

void TeamProposalCards::render()
{
    // The reason being typed keeps the focus across the rebuild.
    QString focused;
    if (const auto *edit = qobject_cast<QLineEdit *>(QApplication::focusWidget()); edit && isAncestorOf(edit) && edit->objectName() == "teamProposalDenyReason")
        focused = edit->property("proposal").toString();
    while (auto *item = m_layout->takeAt(0)) {
        if (auto *w = item->widget()) { w->hide(); w->setParent(nullptr); w->deleteLater(); }
        delete item;
    }
    for (const auto &view : std::as_const(m_views)) m_layout->addWidget(cardFor(view, false));
    for (const auto &view : std::as_const(m_staleViews)) m_layout->addWidget(cardFor(view, true));
    setVisible(m_layout->count() > 0);
    fit();
    if (!m_reveal.isEmpty()) QTimer::singleShot(0, this, [this] { reveal(); });
    if (focused.isEmpty()) return;
    for (auto *edit : findChildren<QLineEdit *>("teamProposalDenyReason"))
        if (edit->property("proposal") == focused) { edit->setFocus(Qt::OtherFocusReason); edit->end(false); }
}

void TeamProposalCards::reveal()
{
    // #754: a refused card grew by its reason; in the lead's chat the cards
    // are short, so its answers scroll into view until the owner answers.
    if (m_reveal.isEmpty()) return;
    for (auto *card : findChildren<QFrame *>("teamProposalCard"))
        if (card->property("proposal") == m_reveal)
            if (auto *allow = card->findChild<QPushButton *>("teamProposalAllow")) m_scroll->ensureWidgetVisible(allow, 0, 6);
}

QWidget *TeamProposalCards::cardFor(const View &view, bool gone)
{
    const Colors c = colorsFor(m_dark);
    // The member permission card's frame (teamMemberPermission).
    auto *box = new QFrame; box->setObjectName("teamProposalCard");
    box->setProperty("team", m_team); box->setProperty("proposal", view.id); box->setProperty("hash", view.hash);
    box->setProperty("kind", view.kind); box->setProperty("stale", gone);
    box->setStyleSheet(QStringLiteral("QFrame#teamProposalCard{background:%1;border:1px solid %2;border-radius:7px;}")
                           .arg(alpha(c.warn, .10).name(QColor::HexArgb), alpha(c.warn, .40).name(QColor::HexArgb)));
    auto *v = new QVBoxLayout(box); v->setContentsMargins(8, 5, 8, 6); v->setSpacing(3);
    auto *head = row(6);
    head->addWidget(new Pill(kindLabel(view.kind), c.warn));
    auto *what = label(view.what, c.fg, 12, true, "teamProposalWhat"); what->setWordWrap(true);
    head->addWidget(what, 1);
    v->addLayout(head);
    const QString when = askedAt(view.at);
    v->addWidget(label(when.isEmpty() ? tr("%1 asks").arg(view.by) : tr("%1 asks, %2").arg(view.by, when), c.text3, 10, false, "teamProposalBy"));
    for (const auto &line : view.lines) {
        auto *change = label(line, c.text2, 11, false, "teamProposalChange");
        change->setWordWrap(true); change->setContentsMargins(6, 0, 0, 0);
        change->setTextInteractionFlags(Qt::TextSelectableByMouse);
        v->addWidget(change);
    }
    if (!view.reason.isEmpty()) {
        auto *reason = label(tr("%1: %2").arg(view.by, view.reason), c.text2, 11, false, "teamProposalReason");
        reason->setWordWrap(true);
        v->addWidget(reason);
    }
    if (const QString notice = m_notices.value(view.id); !notice.isEmpty()) {
        auto *text = label(notice, c.warn, 11, false, "teamProposalNotice"); text->setWordWrap(true);
        v->addWidget(text);
    }
    auto *answers = row(6);
    if (gone) {
        answers->addStretch();
        auto *dismiss = button(tr("Dismiss"), "teamProposalDismiss"); dismiss->setProperty("ghost", true);
        QObject::connect(dismiss, &QPushButton::clicked, this, [this, id = view.id] {
            m_staleViews.remove(id); m_notices.remove(id);
            QTimer::singleShot(0, this, [this] { render(); });
        });
        answers->addWidget(dismiss);
        v->addLayout(answers);
        return box;
    }
    const bool sending = m_sending.contains(view.id);
    if (view.kind == "question") {
        questionCard(view, v);
        return box;
    }
    auto *reasonEdit = new QLineEdit(m_reasons.value(view.id)); reasonEdit->setObjectName("teamProposalDenyReason");
    reasonEdit->setProperty("proposal", view.id);
    reasonEdit->setPlaceholderText(tr("Why deny? The lead receives this"));
    reasonEdit->setMaxLength(2000);
    reasonEdit->setEnabled(!sending);
    QObject::connect(reasonEdit, &QLineEdit::textChanged, this, [this, id = view.id](const QString &text) { m_reasons.insert(id, text); });
    answers->addWidget(reasonEdit, 1);
    if (details && view.kind == "switch-plan-mode") {
        auto *open = button(tr("Details…"), "teamProposalOpen"); open->setProperty("ghost", true);
        open->setToolTip(tr("Open the request's dialog"));
        open->setEnabled(!sending);
        QObject::connect(open, &QPushButton::clicked, this, [this, id = view.id] { QTimer::singleShot(0, this, [this, id] { if (details) details(id); }); });
        answers->addWidget(open);
    }
    auto *deny = button(tr("Deny"), "teamProposalDeny"); deny->setProperty("danger", true);
    auto *allow = button(tr("Allow"), "teamProposalAllow", true);
    deny->setEnabled(!sending && decide); allow->setEnabled(!sending && decide);
    QObject::connect(deny, &QPushButton::clicked, this, [this, view] { answer(view, false); });
    QObject::connect(allow, &QPushButton::clicked, this, [this, view] { answer(view, true); });
    QObject::connect(reasonEdit, &QLineEdit::returnPressed, this, [this, view, reasonEdit] {
        if (!reasonEdit->text().trimmed().isEmpty()) answer(view, false);
    });
    answers->addWidget(deny); answers->addWidget(allow);
    v->addLayout(answers);
    return box;
}

void TeamProposalCards::questionCard(const View &view, QVBoxLayout *v)
{
    // #740: the options as the lead offers them, its recommended one marked,
    // and Other with the owner's own words; Answer sends the one chosen.
    const bool sending = m_sending.contains(view.id);
    auto *group = new QButtonGroup(v->parentWidget());
    for (const auto &option : view.options) {
        auto *pick = new QRadioButton(option == view.recommended ? tr("%1 (recommended)").arg(option) : option);
        pick->setObjectName("teamProposalQuestionOption"); pick->setProperty("option", option);
        pick->setChecked(!m_ownWords.contains(view.id) && m_picked.value(view.id) == option);
        pick->setEnabled(!sending);
        group->addButton(pick);
        v->addWidget(pick);
        QObject::connect(pick, &QRadioButton::toggled, this, [this, id = view.id, option](bool on) {
            if (!on) return;
            m_picked.insert(id, option); m_ownWords.remove(id);
            QTimer::singleShot(0, this, [this] { render(); });
        });
    }
    auto *other = new QRadioButton(view.options.isEmpty() ? tr("Your answer") : tr("Other"));
    other->setObjectName("teamProposalQuestionOther");
    other->setChecked(view.options.isEmpty() || m_ownWords.contains(view.id));
    other->setEnabled(!sending);
    group->addButton(other);
    v->addWidget(other);
    QObject::connect(other, &QRadioButton::toggled, this, [this, id = view.id](bool on) {
        if (!on) return;
        m_ownWords.insert(id);
        QTimer::singleShot(0, this, [this] { render(); });
    });
    auto *answers = row(6);
    const bool own = view.options.isEmpty() || m_ownWords.contains(view.id);
    auto *send = button(tr("Answer"), "teamProposalAnswer", true);
    if (own) {
        auto *words = new QLineEdit(m_reasons.value(view.id)); words->setObjectName("teamProposalAnswerText");
        words->setProperty("proposal", view.id);
        words->setPlaceholderText(tr("Your answer, for the lead"));
        words->setMaxLength(4000); words->setEnabled(!sending);
        QObject::connect(words, &QLineEdit::textChanged, this, [this, id = view.id, send](const QString &text) {
            m_reasons.insert(id, text);
            send->setEnabled(!text.trimmed().isEmpty() && decide);
        });
        QObject::connect(words, &QLineEdit::returnPressed, this, [this, view] { answerQuestion(view); });
        answers->addWidget(words, 1);
    } else answers->addStretch();
    const bool ready = own ? !m_reasons.value(view.id).trimmed().isEmpty() : !m_picked.value(view.id).isEmpty();
    send->setEnabled(ready && !sending && decide);
    QObject::connect(send, &QPushButton::clicked, this, [this, view] { answerQuestion(view); });
    answers->addWidget(send);
    v->addLayout(answers);
}

void TeamProposalCards::answerQuestion(const View &view)
{
    if (!decide || m_sending.contains(view.id) || m_asking) return;
    const bool own = view.options.isEmpty() || m_ownWords.contains(view.id);
    const QString answer = (own ? m_reasons.value(view.id) : m_picked.value(view.id)).trimmed();
    if (answer.isEmpty()) return;
    const QString id = view.id;
    QPointer<TeamProposalCards> guard(this);
    // The question as it is now: a changed or decided one sends nothing.
    if (current) if (const auto *team = current(); team && team->id == m_team) setTeam(team);
    const auto now = std::find_if(m_views.cbegin(), m_views.cend(), [&id](const View &shown) { return shown.id == id; });
    if (now == m_views.cend() || now->hash != view.hash) {
        if (now == m_views.cend()) m_staleViews.insert(id, view);
        m_notices.insert(id, tr("The lead changed or withdrew this question; nothing was sent."));
        QTimer::singleShot(0, this, [this] { render(); });
        return;
    }
    m_sending.insert(id); m_notices.remove(id);
    QTimer::singleShot(0, this, [this] { render(); });
    decide(QJsonObject{{"team", m_team}, {"proposal", id}, {"accept", true}, {"expected_hash", view.hash}, {"answer", answer}},
           [guard, id](bool ok, const QString &message) {
        if (!guard) return;
        auto *self = guard.data();
        if (!ok) {
            self->m_sending.remove(id);
            self->m_notices.insert(id, message.contains(QLatin1String("changed"))
                                           ? tr("The question changed; nothing was sent. Read it again and answer.") : message);
        }
        QTimer::singleShot(0, self, [self] { self->render(); });
    });
}

bool TeamProposalCards::ask(const View &view)
{
    QString title, text, action;
    if (view.kind == "remove-member") {
        title = tr("Remove member?"); text = tr("%1? Its session keeps running; it gets no more tasks.").arg(view.what); action = tr("Remove");
    } else if (view.kind == "swap-member") {
        title = tr("Swap member?");
        text = tr("%1? Its open task goes to the new session with what was done so far; the old session keeps running for you to stop.").arg(view.what);
        action = tr("Swap");
    } else if (view.kind == "cancel-tasks") {
        title = tr("Cancel tasks?"); text = tr("%1?\n%2").arg(view.what, view.lines.join(QLatin1Char('\n'))); action = tr("Cancel tasks");
    } else if (view.kind == "discard-phase") {
        title = tr("Discard phase?"); text = tr("%1? Its tasks are cancelled and the later phases close up.").arg(view.what); action = tr("Discard");
    } else return true;
    const QScopedValueRollback<bool> asking(m_asking, true);
    if (confirm) return confirm(title, text, action);
    QMessageBox dialog(QMessageBox::Warning, title, text, QMessageBox::NoButton, this);
    dialog.setObjectName("teamProposalConfirm"); dialog.setTextFormat(Qt::PlainText);
    auto *ok = dialog.addButton(action, QMessageBox::DestructiveRole);
    auto *cancel = dialog.addButton(QMessageBox::Cancel);
    dialog.setDefaultButton(cancel); dialog.setEscapeButton(cancel);
    dialog.exec();
    return dialog.clickedButton() == ok;
}

void TeamProposalCards::answer(const View &view, bool allow)
{
    if (!decide || m_sending.contains(view.id) || m_asking) return;
    const QString id = view.id;
    m_reveal.clear();
    const bool asked = allow && !ask(view);
    QPointer<TeamProposalCards> guard(this);
    if (!guard) return;
    // A poll during the question was held; the cards catch up now.
    const auto catchUp = [this] { QTimer::singleShot(0, this, [this] { render(); }); };
    if (asked) { catchUp(); return; }
    // The proposal as it is now: a changed or decided one sends nothing.
    if (current) if (const auto *team = current(); team && team->id == m_team) setTeam(team);
    const auto now = std::find_if(m_views.cbegin(), m_views.cend(), [&id](const View &shown) { return shown.id == id; });
    if (now == m_views.cend() || now->hash != view.hash || now->what != view.what || now->lines != view.lines) {
        if (now == m_views.cend()) {
            m_staleViews.insert(id, view);
            m_notices.insert(id, tr("This proposal was decided or withdrawn meanwhile; nothing was sent."));
        } else {
            m_notices.insert(id, tr("The lead changed this proposal while you answered; nothing was sent. Read it again and answer."));
        }
        catchUp();
        return;
    }
    QJsonObject input{{"team", m_team}, {"proposal", id}, {"accept", allow}, {"expected_hash", view.hash}};
    const QString reason = m_reasons.value(id).trimmed();
    if (!allow && !reason.isEmpty()) input.insert("reason", reason);
    m_sending.insert(id); m_notices.remove(id);
    catchUp();
    decide(input, [guard, id, allow](bool ok, const QString &message) {
        if (!guard) return;
        auto *self = guard.data();
        if (!ok) {
            // #754: a refusal always says why on the card, in the server's
            // words (p11's "The plan changed since the warning" was shown as
            // "The proposal changed" and the owner's Allow looked like nothing);
            // the card stays open to answer again.
            self->m_sending.remove(id);
            QString why = message.trimmed();
            while (why.startsWith(QLatin1String("hgs: "))) why = why.mid(5).trimmed();
            if (why.isEmpty()) why = tr("Zerus gave no reason");
            self->m_notices.insert(id, allow ? tr("Allow was refused, nothing was applied: %1").arg(why)
                                             : tr("Deny was refused, nothing was decided: %1").arg(why));
            self->m_reveal = id;
        } else if (const QString wanted = allow ? QStringLiteral("accepted") : QStringLiteral("rejected");
                   (message == "accepted" || message == "rejected") && message != wanted) {
            // A replay returns the stored decision: it was decided before.
            self->m_notices.insert(id, message == "accepted" ? tr("It was already allowed; your Deny changed nothing.")
                                                             : tr("It was already denied; your Allow changed nothing."));
        }
        QTimer::singleShot(0, self, [self] { self->render(); });
    });
}
