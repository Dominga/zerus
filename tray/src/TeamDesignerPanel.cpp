#include "TeamDesignerPanel.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include <QJsonArray>
#include <QJsonDocument>
#include <QScrollArea>
#include <QScrollBar>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
// A value as one line for the diff.
QString shown(const QJsonValue &value)
{
    if (value.isString()) return value.toString();
    if (value.isBool()) return value.toBool() ? QStringLiteral("yes") : QStringLiteral("no");
    if (value.isDouble()) return QString::number(value.toDouble());
    if (value.isArray()) {
        QStringList parts;
        for (const auto &item : value.toArray()) {
            if (item.isObject()) {
                const auto o = item.toObject();
                parts << (o.contains("title") ? o.value("title").toString() + QStringLiteral(" · ") + o.value("role").toString()
                                              : o.value("role").toString() + (o.value("count").toInt(1) > 1 ? QStringLiteral(" × %1").arg(o.value("count").toInt()) : QString()));
            } else parts << shown(item);
        }
        return parts.join(QStringLiteral(", "));
    }
    if (value.isObject()) return QString::fromUtf8(QJsonDocument(value.toObject()).toJson(QJsonDocument::Compact));
    return {};
}
}

TeamDesignerPanel::TeamDesignerPanel(TeamController *teams, QWidget *parent) : QWidget(parent), m_teams(teams)
{
    setObjectName("teamDesignerPanel");
    auto *layout = new QVBoxLayout(this); layout->setContentsMargins(0, 0, 0, 0);
    m_scroll = new QScrollArea; m_scroll->setWidgetResizable(true); m_scroll->setFrameShape(QFrame::NoFrame);
    m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    layout->addWidget(m_scroll);
    QObject::connect(m_teams, &TeamController::catalogChanged, this, [this] { render(); });
}

void TeamDesignerPanel::render()
{
    const Colors c = colorsFor(m_dark);
    const int scroll = m_scroll->verticalScrollBar()->value();
    const auto &catalog = m_teams->catalog();
    auto *content = new QWidget; content->setObjectName("teamDesignerContent");
    auto *v = new QVBoxLayout(content); v->setContentsMargins(12, 12, 12, 12); v->setSpacing(10);
    auto *head = row(8);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("team", c.fg).pixmap(16, 16)); head->addWidget(icon);
    auto *title = label(tr("Draft catalog"), c.fg, 14, true, "teamDesignerTitle"); head->addWidget(title);
    head->addStretch();
    v->addLayout(head);
    int kept = 0, decided = 0;
    for (const auto &p : catalog.proposals) { kept += p.state == "kept"; decided += p.state != "proposed"; }
    const bool stale = !catalog.proposals.isEmpty() && catalog.draftBase != catalog.version;
    auto *meta = label(catalog.proposals.isEmpty()
                           ? tr("Catalog v%1 · no proposals yet. Ask the Team designer in its conversation.").arg(catalog.version)
                           : tr("Based on v%1 · %2 of %3 changes kept · %4 undecided").arg(catalog.draftBase).arg(kept).arg(catalog.proposals.size()).arg(catalog.proposals.size() - decided),
                       stale ? c.danger : c.muted, 12, false, "teamDesignerMeta");
    meta->setWordWrap(true);
    v->addWidget(meta);
    if (stale) {
        auto *note = label(tr("The catalog changed since these proposals (now v%1). Discard the draft and ask again.").arg(catalog.version), c.danger, 12);
        note->setWordWrap(true); v->addWidget(note);
    }
    v->addWidget(label(tr("The designer can only propose. Nothing changes before you apply; running teams keep their roster."), c.muted, 11));
    if (!m_notice.isEmpty()) { auto *n = label(m_notice, c.danger, 12, false, "teamDesignerNotice"); n->setWordWrap(true); v->addWidget(n); }
    auto *actions = row(6);
    auto *discard = button(tr("Discard draft"), "teamDesignerDiscard");
    discard->setEnabled(!catalog.proposals.isEmpty());
    QObject::connect(discard, &QPushButton::clicked, this, [this] {
        if (confirm && !confirm("teamDesignerDiscardConfirm", tr("Discard draft?"), tr("Discard every proposal of this draft? The catalog stays as it is."), tr("Discard draft"))) return;
        m_teams->call(QStringLiteral("catalog discard"), {}, [this](bool ok, const QJsonObject &, const QString &error) { m_notice = ok ? QString() : error; render(); });
    });
    auto *apply = button(tr("Apply %n change(s)", nullptr, kept), "teamDesignerApply", true);
    apply->setEnabled(kept > 0 && !stale);
    apply->setToolTip(kept ? tr("Write catalog v%1 with the kept changes").arg(catalog.version + 1) : tr("Keep at least one change"));
    QObject::connect(apply, &QPushButton::clicked, this, [this, kept, version = catalog.version] {
        if (confirm && !confirm("teamDesignerApplyConfirm", tr("Apply changes?"),
                                tr("Apply %1 kept change(s) as catalog v%2? Rejected and undecided proposals are dropped.").arg(kept).arg(version + 1), tr("Apply"))) return;
        m_teams->call(QStringLiteral("catalog apply"), {}, [this](bool ok, const QJsonObject &, const QString &error) { m_notice = ok ? QString() : error; render(); });
    });
    actions->addWidget(discard); actions->addStretch(); actions->addWidget(apply);
    v->addLayout(actions);
    // Kinds as tabs with their counts.
    auto *tabs = row(4);
    for (const auto &[kind, name] : QList<std::pair<QString, QString>>{{"role", tr("Roles")}, {"template", tr("Templates")}, {"rule", tr("Routing")}}) {
        int n = 0; for (const auto &p : catalog.proposals) n += p.kind == kind;
        auto *b = button(QStringLiteral("%1 %2").arg(name).arg(n), "teamDesignerKind");
        b->setCheckable(true); b->setChecked(m_kind == kind); b->setProperty("kind", kind);
        QObject::connect(b, &QPushButton::clicked, this, [this, kind = kind] { m_kind = kind; render(); });
        tabs->addWidget(b);
    }
    tabs->addStretch();
    v->addLayout(tabs);
    int shownCount = 0;
    for (const auto &p : catalog.proposals) if (p.kind == m_kind) { v->addWidget(proposalCard(p)); ++shownCount; }
    if (!shownCount) v->addWidget(label(tr("No proposals of this kind."), c.muted, 12));
    // What the catalog would be with the kept changes.
    const auto preview = withProposals(catalog, false);
    v->addWidget(label(tr("After Apply"), c.text3, 12, true));
    QStringList names;
    if (m_kind == "role") for (const auto &r : preview.roles) names << r.name;
    else if (m_kind == "template") for (const auto &t : preview.templates) names << t.name;
    else for (const auto &r : preview.routing) names << QStringLiteral("“%1” → %2").arg(r.when, r.templateId);
    auto *list = label(names.join(QStringLiteral(" · ")), c.text2, 12, false, "teamDesignerPreview"); list->setWordWrap(true);
    v->addWidget(list);
    v->addStretch();
    retire(m_scroll->takeWidget());   // a click inside it may be running
    m_scroll->setWidget(content);
    m_scroll->verticalScrollBar()->setValue(scroll);
}

QWidget *TeamDesignerPanel::proposalCard(const Proposal &p)
{
    const Colors c = colorsFor(m_dark);
    const auto &catalog = m_teams->catalog();
    auto *box = card(c, p.state == "kept" ? c.accent : QColor());
    box->setObjectName("teamProposal");
    box->setProperty("proposal", p.id);
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 8, 10, 8); v->setSpacing(4);
    auto *top = row(6);
    const QString mark = p.action == "add" ? tr("NEW") : p.action == "remove" ? tr("REMOVED") : tr("CHANGED");
    top->addWidget(new Pill(mark, p.action == "remove" ? c.danger : p.action == "add" ? c.accent : c.folder));
    auto *title = elided(p.title.isEmpty() ? QStringLiteral("%1 %2").arg(p.kind, p.target) : p.title, c.fg, 12);
    QFont f = title->font(); f.setWeight(QFont::DemiBold); title->setFont(f);
    if (p.state == "rejected") { f.setStrikeOut(true); title->setFont(f); }
    top->addWidget(title, 1);
    v->addLayout(top);
    if (!p.reason.isEmpty()) { auto *reason = label(p.reason, c.text2, 12); reason->setWordWrap(true); v->addWidget(reason); }
    // Field by field: the old value struck through, the new one tinted.
    QJsonObject before;
    if (p.kind == "role") { if (const auto *r = catalog.role(p.target)) before = r->json(); }
    else if (p.kind == "template") { if (const auto *t = catalog.templ(p.target)) before = t->json(); }
    else for (const auto &r : catalog.routing) if (r.id == p.target) before = r.json();
    const QJsonObject after = p.action == "remove" ? QJsonObject() : p.value;
    QStringList keys = before.keys(); for (const auto &k : after.keys()) if (!keys.contains(k)) keys << k;
    for (const auto &key : keys) {
        if (key == "id") continue;
        const QString old = shown(before.value(key)), next = shown(after.value(key));
        if (old == next) continue;
        auto *r = row(6);
        auto *name = label(key, c.muted, 11); name->setFixedWidth(90); r->addWidget(name, 0, Qt::AlignTop);
        auto *values = new QVBoxLayout; values->setSpacing(1);
        if (!old.isEmpty()) {
            auto *o = label(old, c.text2, 11); QFont of = o->font(); of.setStrikeOut(true); o->setFont(of); o->setWordWrap(true);
            o->setStyleSheet(QStringLiteral("background:%1;").arg(alpha(c.danger, .12).name(QColor::HexArgb)));
            values->addWidget(o);
        }
        if (!next.isEmpty()) {
            auto *n = label(next, c.fg, 11); n->setWordWrap(true);
            n->setStyleSheet(QStringLiteral("background:%1;").arg(alpha(c.accent, .12).name(QColor::HexArgb)));
            values->addWidget(n);
        }
        r->addLayout(values, 1);
        v->addLayout(r);
    }
    auto *buttons = row(6);
    buttons->addWidget(label(p.state == "kept" ? tr("Kept") : p.state == "rejected" ? tr("Rejected") : tr("Undecided"),
                             p.state == "kept" ? c.accent : p.state == "rejected" ? c.danger : c.muted, 11));
    buttons->addStretch();
    const auto decide = [this, id = p.id](const QString &decision) {
        m_teams->call(QStringLiteral("catalog decide"), {{"proposal", id}, {"decision", decision}},
                      [this](bool ok, const QJsonObject &, const QString &error) { m_notice = ok ? QString() : error; if (!ok) render(); });
    };
    if (p.state == "proposed") {
        auto *reject = button(tr("Reject"), "teamProposalReject"); QObject::connect(reject, &QPushButton::clicked, this, [decide] { decide("reject"); });
        auto *keep = button(tr("Keep"), "teamProposalKeep", true); QObject::connect(keep, &QPushButton::clicked, this, [decide] { decide("keep"); });
        buttons->addWidget(reject); buttons->addWidget(keep);
    } else {
        auto *undo = button(tr("Undo"), "teamProposalUndo"); QObject::connect(undo, &QPushButton::clicked, this, [decide] { decide("undo"); });
        buttons->addWidget(undo);
    }
    v->addLayout(buttons);
    return box;
}
