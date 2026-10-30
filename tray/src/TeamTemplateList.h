#pragma once
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QSettings>
#include <QToolButton>
#include <QVBoxLayout>
#include <algorithm>
#include <functional>
#include "Settings.h"
#include "TeamModel.h"
#include "TeamWidgets.h"

// WhiteWh fork: agent teams, F6. Many templates in a compact list: a search
// field (name, description and the roles a template seats), favourites first
// with a star and a favourites-only filter, one line per template and "No
// template" last. The big card belongs to the form, for the selected template
// only. The list never picks for the form: `selected` runs only when the user
// chooses a row, never while the list is built, refilled or destroyed.
class TeamTemplateList : public QWidget {
public:
    explicit TeamTemplateList(QWidget *parent = nullptr) : QWidget(parent)
    {
        setObjectName("teamTemplateListBox");
        auto *v = new QVBoxLayout(this); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
        auto *head = TeamUi::row(6);
        m_search = new QLineEdit; m_search->setObjectName("teamTemplateSearch");
        m_search->setPlaceholderText(tr("Search templates or roles…")); m_search->setClearButtonEnabled(true);
        head->addWidget(m_search, 1);
        m_favouritesOnly = new QToolButton; m_favouritesOnly->setObjectName("teamTemplateFavouritesOnly");
        m_favouritesOnly->setText(QStringLiteral("★")); m_favouritesOnly->setCheckable(true);
        m_favouritesOnly->setToolTip(tr("Show favourites only")); m_favouritesOnly->setCursor(Qt::PointingHandCursor);
        head->addWidget(m_favouritesOnly);
        v->addLayout(head);
        m_list = new QListWidget; m_list->setObjectName("teamTemplateList");
        m_list->setSelectionMode(QAbstractItemView::SingleSelection);
        m_list->setFrameShape(QFrame::NoFrame);
        m_list->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        v->addWidget(m_list);
        m_detail = new QLabel; m_detail->setObjectName("teamTemplateDetail"); m_detail->setWordWrap(true);
        m_detail->setTextFormat(Qt::PlainText); m_detail->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        v->addWidget(m_detail);
        v->addStretch(1);
        setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Maximum);
        QObject::connect(m_search, &QLineEdit::textChanged, this, [this] { rebuild(); });
        QObject::connect(m_favouritesOnly, &QToolButton::toggled, this, [this] { rebuild(); });
        QObject::connect(m_list, &QListWidget::currentItemChanged, this, [this](QListWidgetItem *item) {
            if (m_rebuilding || !item) return;
            m_current = item->data(Qt::UserRole).toString();
            updateDetail();
            if (selected) selected(m_current);
        });
        setDark(true);
    }
    ~TeamTemplateList() override
    {
        // The rows go after this body: nothing may call back into the form.
        selected = nullptr;
        if (m_list) QObject::disconnect(m_list, nullptr, this, nullptr);
    }

    // A template was chosen: its id, or "-" for no template.
    std::function<void(const QString &id)> selected;

    void setCatalog(const Teams::Catalog &catalog) { m_catalog = catalog; rebuild(); }
    QString current() const { return m_current; }
    // Marks a row without telling the form; "-" or empty is "No template".
    void setCurrent(const QString &id) { m_current = id.isEmpty() ? QStringLiteral("-") : id; rebuild(); }
    // The form that shows the selected template's card hides the detail line.
    void setDetailVisible(bool on) { m_detail->setVisible(on); }
    void setDark(bool dark)
    {
        m_dark = dark;
        const auto c = TeamUi::colorsFor(dark);
        // QA #347: the list paints its own surface and its rows take the theme's
        // foreground. A transparent list over a light viewport palette showed the
        // dark theme's row text on white.
        QPalette view = m_list->palette();
        view.setColor(QPalette::Base, c.surface); view.setColor(QPalette::Window, c.surface);
        view.setColor(QPalette::Text, c.fg); view.setColor(QPalette::WindowText, c.fg);
        view.setColor(QPalette::Highlight, TeamUi::alpha(c.accent, .16));
        view.setColor(QPalette::HighlightedText, c.fg);
        m_list->setPalette(view);
        m_list->viewport()->setAutoFillBackground(true);
        m_list->viewport()->setPalette(view);
        setStyleSheet(QStringLiteral(
            "QListWidget#teamTemplateList{background:%7;border:1px solid %1;border-radius:8px;outline:0;}"
            "QListWidget#teamTemplateList::item{border-bottom:1px solid %2;}"
            "QListWidget#teamTemplateList::item:selected,QListWidget#teamTemplateList::item:selected:active{background:%3;}"
            "QListWidget#teamTemplateList::item:hover:!selected{background:%4;}"
            "QLabel#teamTemplateName{color:%8;}"
            "QLabel#teamTemplateMeta{color:%9;}"
            "QLineEdit#teamTemplateSearch{background:%10;color:%8;border:1px solid %1;border-radius:6px;padding:3px 6px;}"
            "QToolButton#teamTemplateFavouritesOnly{padding:2px 7px;border:1px solid %1;border-radius:6px;color:%5;}"
            "QToolButton#teamTemplateFavouritesOnly:checked{color:%6;border-color:%6;}"
            "QLabel#teamTemplateDetail{color:%5;font-size:11px;}")
            .arg(c.border.name(), TeamUi::alpha(c.border, .5).name(QColor::HexArgb), TeamUi::alpha(c.accent, dark ? .16 : .12).name(QColor::HexArgb),
                 TeamUi::alpha(c.fg, .04).name(QColor::HexArgb), c.muted.name(), favouriteColor(dark).name(), c.surface.name(),
                 c.fg.name(), c.dim.name(), c.card.name()));
        rebuild();
    }
    // The ids in list order: every template the search keeps, favourites
    // first, then "-".
    QStringList visibleIds() const
    {
        QStringList ids;
        for (int i = 0; i < m_list->count(); ++i) ids << m_list->item(i)->data(Qt::UserRole).toString();
        return ids;
    }
    static QStringList favourites() { return Settings::value(QStringLiteral("teams/templateFavourites")).toStringList(); }
    static bool isFavourite(const QString &id) { return favourites().contains(id); }
    void toggleFavourite(const QString &id)
    {
        auto ids = favourites();
        if (ids.contains(id)) ids.removeAll(id); else ids << id;
        Settings::set(QStringLiteral("teams/templateFavourites"), ids);
        rebuild();
    }

private:
    static QColor favouriteColor(bool dark) { return QColor(dark ? "#e8c46a" : "#a87a00"); }
    int seats(const Teams::Template &t) const { int n = 0; for (const auto &seat : t.roster) n += seat.count; return n; }
    QString roleName(const QString &id) const { const auto *role = m_catalog.role(id); return role ? role->name : id; }
    void rebuild()
    {
        m_rebuilding = true;
        // QA #347: an old row's item widget outlives clear(); delete it so no
        // stale row paints over a new one after a theme switch. Hide it first:
        // a deferred delete posted before a dialog's own event loop runs only
        // after that loop ends, so the row stayed on top of the first new one
        // (KDE, the empty-catalog "No template" over the first template).
        for (int i = 0; i < m_list->count(); ++i) {
            if (auto *item = m_list->item(i)) {
                if (auto *widget = m_list->itemWidget(item)) { widget->hide(); m_list->removeItemWidget(item); widget->deleteLater(); }
            }
        }
        m_list->clear();
        const auto c = TeamUi::colorsFor(m_dark);
        const QString query = m_search->text().trimmed();
        const auto favs = favourites();
        QList<const Teams::Template *> shown;
        for (const auto &t : m_catalog.templates) {
            bool matches = query.isEmpty() || t.name.contains(query, Qt::CaseInsensitive) || t.id.contains(query, Qt::CaseInsensitive)
                || t.description.contains(query, Qt::CaseInsensitive);
            for (const auto &seat : t.roster)
                matches = matches || seat.role.contains(query, Qt::CaseInsensitive) || roleName(seat.role).contains(query, Qt::CaseInsensitive);
            if (!matches || (m_favouritesOnly->isChecked() && !favs.contains(t.id))) continue;
            shown << &t;
        }
        std::stable_sort(shown.begin(), shown.end(), [&favs](const Teams::Template *a, const Teams::Template *b) {
            const bool fa = favs.contains(a->id), fb = favs.contains(b->id);
            return fa != fb ? fa : a->name.localeAwareCompare(b->name) < 0;
        });
        const auto addRow = [&](const QString &id, const QString &name, const QString &meta, const QString &tip, bool favourite) {
            auto *item = new QListWidgetItem(m_list);
            item->setData(Qt::UserRole, id);
            item->setData(Qt::AccessibleTextRole, meta.isEmpty() ? name : name + QStringLiteral(", ") + meta);
            item->setToolTip(tip);
            item->setSizeHint(QSize(0, rowHeight));
            // One line: the name, its size, and the star at the end. The row
            // ignores clicks outside the star, so the list selects the row.
            auto *line = new QWidget; line->setObjectName("teamTemplateRow"); line->setProperty("template", id);
            auto *h = new QHBoxLayout(line); h->setContentsMargins(10, 0, 4, 0); h->setSpacing(8);
            h->addWidget(TeamUi::label(name, c.fg, 12, false, "teamTemplateName"));
            if (!meta.isEmpty()) h->addWidget(TeamUi::label(meta, c.dim, 11, false, "teamTemplateMeta"));
            h->addStretch();
            if (id != QLatin1String("-")) {
                auto *star = new QToolButton; star->setObjectName("teamTemplateFavourite"); star->setProperty("template", id);
                star->setText(favourite ? QStringLiteral("★") : QStringLiteral("☆")); star->setAutoRaise(true);
                star->setCursor(Qt::PointingHandCursor);
                star->setToolTip(favourite ? tr("Remove from favourites") : tr("Add to favourites"));
                star->setStyleSheet(QStringLiteral("QToolButton{border:0;background:transparent;color:%1;font-size:14px;padding:0 4px;}")
                                        .arg((favourite ? favouriteColor(m_dark) : c.text3).name()));
                QObject::connect(star, &QToolButton::clicked, this, [this, id] { QMetaObject::invokeMethod(this, [this, id] { toggleFavourite(id); }, Qt::QueuedConnection); });
                h->addWidget(star);
            }
            m_list->setItemWidget(item, line);
            if (id == m_current) m_list->setCurrentItem(item);
        };
        for (const auto *t : shown) {
            const int n = seats(*t);
            QString meta = n == 1 ? tr("1 member") : tr("%1 members").arg(n);
            if (!t->tasks.isEmpty()) meta += QStringLiteral(" · ") + (t->tasks.size() == 1 ? tr("1 task") : tr("%1 tasks").arg(t->tasks.size()));
            addRow(t->id, t->name, meta, t->description.isEmpty() ? t->name : t->description, favs.contains(t->id));
        }
        // "No template" is a real choice, kept last.
        addRow(QStringLiteral("-"), tr("No template"), tr("the lead drafts the team"), tr("Start empty: the lead drafts members and tasks from the goal"), false);
        m_list->setFixedHeight(qMin(m_list->count(), maxRows) * rowHeight + 2);
        m_rebuilding = false;
        updateDetail();
    }
    void updateDetail()
    {
        const auto *t = m_catalog.templ(m_current);
        if (!t) { m_detail->setText(tr("No template: the lead drafts members and tasks from the goal.")); return; }
        QStringList roles;
        for (const auto &seat : t->roster) roles << (seat.count > 1 ? QStringLiteral("%1 ×%2").arg(roleName(seat.role)).arg(seat.count) : roleName(seat.role));
        QString text = t->name + QStringLiteral(" — ") + (roles.isEmpty() ? tr("no members") : roles.join(QStringLiteral(", ")));
        if (!t->description.isEmpty()) text += QLatin1Char('\n') + t->description;
        m_detail->setText(text);
    }
    static constexpr int rowHeight = 30, maxRows = 6;
    Teams::Catalog m_catalog;
    QLineEdit *m_search = nullptr;
    QToolButton *m_favouritesOnly = nullptr;
    QListWidget *m_list = nullptr;
    QLabel *m_detail = nullptr;
    QString m_current = QStringLiteral("-");
    bool m_dark = true, m_rebuilding = false;
};
