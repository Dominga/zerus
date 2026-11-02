#include "Settings.h"
#include "JsonParse.h"
#include "ProcessRunner.h"
#include "TeamCatalogEditor.h"
#include "TeamController.h"
#include "HgsClient.h"
#include "TeamWidgets.h"
#include "CollapsibleBlock.h"
#include <QCheckBox>
#include <QComboBox>
#include <QFile>
#include <QFileDialog>
#include <QGraphicsOpacityEffect>
#include <QGridLayout>
#include <QJsonArray>
#include <QJsonDocument>
#include <QKeyEvent>
#include <QLineEdit>
#include <QListWidget>
#include <QPainter>
#include <QPainterPath>
#include <QPlainTextEdit>
#include <QRadioButton>
#include <QRegularExpressionValidator>
#include <QSpinBox>
#include <QAbstractTextDocumentLayout>
#include <QRegularExpression>
#include <QScrollBar>
#include <QScrollArea>
#include <QSettings>
#include <QToolButton>
#include <QTransform>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
const QMap<QString, QString> agentNames{{"claude", "Claude Code"}, {"codex", "Codex"}, {"kimi", "Kimi"}, {"dsh", "DeepSeek"}, {"opencode", "OpenCode"}};
// U25 (spec 11.3): the 16 colors of the role palette, in the prototype's
// order, and its 10 role icons. A role stores the palette name; a #hex of an
// older catalog still shows.
const QStringList swatches{"lead", "architect", "backend", "frontend", "tester", "reviewer", "security", "integrator",
                           "researcher", "secretary", "coder", "qa", "designer", "leadgpt", "expert", "proexpert"};
const QStringList roleIcons{"lead", "architect", "backend", "frontend", "tester", "reviewer", "security", "integrator", "researcher", "secretary"};
// The five dangerous actions (spec 12 danger), in the prototype's order.
const QList<std::pair<QString, const char *>> dangers{{"push", QT_TRANSLATE_NOOP("TeamCatalogEditor", "git push to the remote")},
                                                      {"deploy", QT_TRANSLATE_NOOP("TeamCatalogEditor", "Deploy or release")},
                                                      {"del", QT_TRANSLATE_NOOP("TeamCatalogEditor", "Delete files outside its folder")},
                                                      {"pkgs", QT_TRANSLATE_NOOP("TeamCatalogEditor", "Install packages")},
                                                      {"net", QT_TRANSLATE_NOOP("TeamCatalogEditor", "Network access beyond the provider")}};
const QStringList fixedBlocks{"rules", "role", "protocol", "task"};
const QString defaultTaskTemplate = QStringLiteral("#{id} {title}\n{description}\nAcceptance:\n{criteria}\nReport with: hgs team report --task {id}");
constexpr int listWidth = 230, labelWidth = 120, rowHeight = 30, templateRowHeight = 48, pipelineRowHeight = 72;

QString tr(const char *text) { return QCoreApplication::translate("TeamCatalogEditor", text); }

QComboBox *choice(const QList<std::pair<QString, QString>> &items, const QString &current, const char *name)
{
    auto *box = new QComboBox; box->setObjectName(name);
    for (const auto &[value, text] : items) box->addItem(text, value);
    if (box->findData(current) < 0 && !current.isEmpty()) box->addItem(current, current);
    box->setCurrentIndex(qMax(0, box->findData(current)));
    return box;
}

QString slug(const QString &name)
{
    QString id;
    for (const auto ch : name.toLower()) id += ch.isLetterOrNumber() && ch.unicode() < 128 ? ch : QChar('-');
    while (id.contains("--")) id.replace("--", "-");
    id = id.left(40);
    while (id.startsWith('-')) id.remove(0, 1);
    while (id.endsWith('-')) id.chop(1);
    return id.isEmpty() ? QStringLiteral("item") : id;
}

// English plurals without a translation file: "1 change", "3 changes".
QString counted(int n, const QString &one, const QString &many) { return (n == 1 ? one : many).arg(n); }

// A palette name or a color of an older catalog.
QColor swatchColor(const QString &name, bool dark)
{
    if (const QColor plain(name); plain.isValid() && name.startsWith('#')) return plain;
    return WorkspaceSkin::color(WorkspaceSkin::active(), dark, QStringLiteral("role") + name.left(1).toUpper() + name.mid(1));
}

QColor colorOf(const Teams::Role &role, bool dark)
{
    if (role.color.isEmpty()) return roleColor(role.name.isEmpty() ? role.id : role.name, dark);
    if (const QColor plain(role.color); plain.isValid()) return plain;
    return swatchColor(role.color, dark);
}

// The role's icon: its own (r_backend in spec 12), else the one its id or name names.
QString iconOf(const Teams::Role &role)
{
    QString icon = role.icon; if (icon.startsWith(QLatin1String("r_"))) icon = icon.mid(2);
    if (roleIcons.contains(icon)) return icon;
    const QString key = Teams::roleColorKey(role.id) + ' ' + Teams::roleColorKey(role.name);
    for (const auto &name : roleIcons) if (key.contains(name)) return name;
    if (key.contains("qa")) return QStringLiteral("tester");
    if (key.contains("coder")) return QStringLiteral("backend");
    if (key.contains("designer")) return QStringLiteral("frontend");
    if (key.contains("expert")) return QStringLiteral("reviewer");
    return QStringLiteral("architect");
}

// The prototype's role icons, drawn in a 12 px box.
void drawRoleIcon(QPainter &p, const QRectF &box, const QColor &color, const QString &kind)
{
    p.save();
    p.setRenderHint(QPainter::Antialiasing);
    QPen pen(color, 1.2); pen.setCapStyle(Qt::RoundCap); pen.setJoinStyle(Qt::RoundJoin); p.setPen(pen); p.setBrush(Qt::NoBrush);
    const double x = box.x(), y = box.y(), w = box.width(), h = box.height();
    const auto at = [&](double fx, double fy) { return QPointF(x + fx * w, y + fy * h); };
    QPainterPath path;
    if (kind == "lead") {
        path.moveTo(at(.12, .72)); path.lineTo(at(.12, .3)); path.lineTo(at(.35, .52)); path.lineTo(at(.5, .2)); path.lineTo(at(.65, .52));
        path.lineTo(at(.88, .3)); path.lineTo(at(.88, .72)); path.closeSubpath(); p.drawPath(path); p.drawLine(at(.15, .86), at(.85, .86));
    } else if (kind == "architect") {
        p.drawEllipse(QRectF(at(.4, .08), at(.6, .28))); p.drawLine(at(.46, .28), at(.2, .9)); p.drawLine(at(.54, .28), at(.8, .9)); p.drawLine(at(.3, .64), at(.7, .64));
    } else if (kind == "backend") {
        p.drawRoundedRect(QRectF(at(.12, .14), at(.88, .46)), 1.5, 1.5); p.drawRoundedRect(QRectF(at(.12, .54), at(.88, .86)), 1.5, 1.5);
        p.drawLine(at(.26, .3), at(.34, .3)); p.drawLine(at(.26, .7), at(.34, .7));
    } else if (kind == "frontend") {
        p.drawRoundedRect(QRectF(at(.1, .14), at(.9, .86)), 1.5, 1.5); p.drawLine(at(.1, .36), at(.9, .36)); p.drawLine(at(.36, .36), at(.36, .86));
    } else if (kind == "tester") {
        path.moveTo(at(.38, .1)); path.lineTo(at(.38, .42)); path.lineTo(at(.14, .86)); path.lineTo(at(.86, .86)); path.lineTo(at(.62, .42)); path.lineTo(at(.62, .1));
        p.drawPath(path); p.drawLine(at(.3, .1), at(.7, .1)); p.drawLine(at(.26, .64), at(.74, .64));
    } else if (kind == "reviewer") {
        p.drawEllipse(QRectF(at(.12, .12), at(.64, .64))); p.drawLine(at(.58, .58), at(.88, .88));
    } else if (kind == "security") {
        path.moveTo(at(.5, .08)); path.lineTo(at(.86, .22)); path.cubicTo(at(.86, .58), at(.7, .8), at(.5, .92)); path.cubicTo(at(.3, .8), at(.14, .58), at(.14, .22));
        path.closeSubpath(); p.drawPath(path); p.drawLine(at(.36, .5), at(.47, .62)); p.drawLine(at(.47, .62), at(.66, .4));
    } else if (kind == "integrator") {
        p.drawEllipse(QRectF(at(.16, .08), at(.36, .28))); p.drawEllipse(QRectF(at(.64, .08), at(.84, .28))); p.drawEllipse(QRectF(at(.4, .72), at(.6, .92)));
        path.moveTo(at(.26, .28)); path.cubicTo(at(.26, .5), at(.5, .5), at(.5, .72)); path.moveTo(at(.74, .28)); path.cubicTo(at(.74, .5), at(.5, .5), at(.5, .72));
        p.drawPath(path);
    } else if (kind == "researcher") {
        p.drawRoundedRect(QRectF(at(.08, .18), at(.5, .84)), 1, 1); p.drawRoundedRect(QRectF(at(.5, .18), at(.92, .84)), 1, 1);
    } else {   // secretary: a pencil
        path.moveTo(at(.16, .84)); path.lineTo(at(.2, .64)); path.lineTo(at(.66, .18)); path.lineTo(at(.82, .34)); path.lineTo(at(.36, .8)); path.closeSubpath();
        p.drawPath(path); p.drawLine(at(.58, .26), at(.74, .42));
    }
    p.restore();
}

QIcon roleIcon(const QString &kind, const QColor &color)
{
    QPixmap pixmap(32, 32); pixmap.setDevicePixelRatio(2.0); pixmap.fill(Qt::transparent);
    QPainter p(&pixmap); drawRoleIcon(p, QRectF(1, 1, 14, 14), color, kind);
    return QIcon(pixmap);
}

// A role badge: the role's icon and name in its color; dashed with "?" when
// the role joins only if needed; dimmed when a template does not use it.
class RoleChip : public QWidget {
public:
    RoleChip(const QString &text, const QColor &color, const QString &icon, bool dashed = false, int width = 0)
        : m_text(text), m_color(color), m_icon(icon), m_dashed(dashed)
    {
        QFont f = font(); f.setPixelSize(12); f.setWeight(QFont::DemiBold); setFont(f);
        setToolTip(text);
        if (width) setFixedWidth(width); else setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
    }
    QSize sizeHint() const override { return {fontMetrics().horizontalAdvance(m_text) + 32, 21}; }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        const QRectF r = QRectF(rect()).adjusted(.5, 1.5, -.5, -1.5);
        QPen pen(alpha(m_color, .55), 1); if (m_dashed) pen.setStyle(Qt::DashLine);
        p.setPen(pen); p.setBrush(alpha(m_color, .12)); p.drawRoundedRect(r, r.height() / 2, r.height() / 2);
        drawRoleIcon(p, QRectF(7, (height() - 11) / 2.0, 11, 11), m_color, m_icon);
        p.setPen(m_color);
        p.drawText(QRect(22, 0, width() - 28, height()), Qt::AlignVCenter | Qt::AlignLeft, fontMetrics().elidedText(m_text, Qt::ElideRight, width() - 28));
    }
private:
    QString m_text; QColor m_color; QString m_icon; bool m_dashed;
};

RoleChip *chipOf(const Teams::Role &role, bool dark, const QString &suffix = {}, bool dashed = false, int width = 0)
{
    auto *chip = new RoleChip(role.name + suffix, colorOf(role, dark), iconOf(role), dashed, width);
    chip->setObjectName("teamRoleChip"); chip->setProperty("role", role.id);
    return chip;
}

// − n +: a count the prototype steps with buttons.
class Stepper : public QWidget {
public:
    Stepper(int value, int low, int high, const char *name, std::function<void(int)> changed)
        : m_value(value), m_low(low), m_high(high), m_changed(std::move(changed))
    {
        setObjectName(name);
        auto *h = new QHBoxLayout(this); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(0);
        m_minus = new QPushButton(QStringLiteral("−")); m_minus->setObjectName("teamStepperMinus");
        m_label = new QLabel; m_label->setObjectName("teamStepperValue"); m_label->setAlignment(Qt::AlignCenter); m_label->setFixedWidth(26);
        m_plus = new QPushButton(QStringLiteral("+")); m_plus->setObjectName("teamStepperPlus");
        for (auto *b : {m_minus, m_plus}) { b->setFixedSize(26, 26); b->setAutoDefault(false); b->setCursor(Qt::PointingHandCursor); b->setProperty("stepper", true); }
        h->addWidget(m_minus); h->addWidget(m_label); h->addWidget(m_plus);
        connect(m_minus, &QPushButton::clicked, this, [this] { set(m_value - 1); });
        connect(m_plus, &QPushButton::clicked, this, [this] { set(m_value + 1); });
        setProperty("stepperBox", true); setAttribute(Qt::WA_StyledBackground);
        display(m_value);
        setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
    }
    int value() const { return m_value; }
private:
    void set(int value)
    {
        value = qBound(m_low, value, m_high);
        if (value == m_value) return;
        m_value = value; display(value);
        if (m_changed) m_changed(value);
    }
    void display(int value) { m_label->setText(QString::number(value)); m_minus->setEnabled(value > m_low); m_plus->setEnabled(value < m_high); }
    int m_value, m_low, m_high;
    std::function<void(int)> m_changed;
    QPushButton *m_minus, *m_plus; QLabel *m_label;
};

// Specialization: Backspace in the empty field removes the last chip.
class SkillEdit : public QLineEdit {
public:
    std::function<void()> backspaceEmpty;
protected:
    void keyPressEvent(QKeyEvent *event) override
    {
        if (event->key() == Qt::Key_Backspace && text().isEmpty() && backspaceEmpty) { backspaceEmpty(); return; }
        QLineEdit::keyPressEvent(event);
    }
};

// A text area that grows with its text up to `lines`.
QPlainTextEdit *area(const QString &text, const char *name, int minLines = 2, int maxLines = 8)
{
    auto *edit = new QPlainTextEdit(text); edit->setObjectName(name);
    edit->setTabChangesFocus(true);
    const auto fit = [edit, minLines, maxLines] {
        const int line = edit->fontMetrics().lineSpacing();
        const int count = int(edit->document()->size().height());
        const int lines = qBound(minLines, count, maxLines);
        edit->setVerticalScrollBarPolicy(count > maxLines ? Qt::ScrollBarAsNeeded : Qt::ScrollBarAlwaysOff);
        edit->setFixedHeight(lines * line + 2 * int(edit->document()->documentMargin()) + 16);
    };
    QObject::connect(edit->document()->documentLayout(), &QAbstractTextDocumentLayout::documentSizeChanged, edit, fit);
    fit();
    return edit;
}

// Remembered fold of a catalog block, under the one collapsible block's key
// (spec 1.5); its header sits in the label column, as in the prototype.
bool folded(const QString &key) { return !Settings::value(CollapsibleBlock::settingsKey(QStringLiteral("catalog-") + key), true).toBool(); }
void setFolded(const QString &key, bool on) { Settings::set(CollapsibleBlock::settingsKey(QStringLiteral("catalog-") + key), !on); }

QString markText(const Teams::Proposal &p)
{
    const QString text = p.action == "add" ? QStringLiteral("NEW") : p.action == "remove" ? QStringLiteral("REMOVED") : QStringLiteral("CHANGED");
    return p.state == "kept" ? text + QStringLiteral(" · kept") : text;
}

QLabel *mark(const Teams::Proposal &p, const Colors &c)
{
    if (p.state == "rejected") {
        auto *l = new QLabel(QStringLiteral("rejected")); l->setObjectName("teamCatalogMark");
        l->setStyleSheet(QStringLiteral("color:%1;background:%2;border-radius:4px;padding:0 5px;font-size:10px;font-weight:600;")
                             .arg(c.dim.name(), alpha(c.dim, .15).name(QColor::HexArgb)));
        return l;
    }
    const QColor color = p.action == "add" ? c.accent : p.action == "remove" ? c.danger : c.folder;
    auto *l = new QLabel(markText(p)); l->setObjectName("teamCatalogMark");
    l->setStyleSheet(QStringLiteral("color:%1;background:%2;border:1px solid %3;border-radius:4px;padding:0 5px;font-size:10px;font-weight:700;")
                         .arg(color.name(), alpha(color, .10).name(QColor::HexArgb), alpha(color, .55).name(QColor::HexArgb)));
    return l;
}

// A field label with a warn dot when the field has an unsaved edit.
QLabel *fieldLabel(const QString &text, bool edited, const Colors &c)
{
    auto *l = new QLabel(edited ? QStringLiteral("%1 <span style='color:%2;'>●</span>").arg(text.toHtmlEscaped(), c.warn.name()) : text.toHtmlEscaped());
    l->setTextFormat(Qt::RichText); l->setFixedWidth(labelWidth); l->setAlignment(Qt::AlignRight | Qt::AlignTop);
    l->setStyleSheet(QStringLiteral("color:%1;font-size:12px;padding-top:6px;").arg(c.label.name()));
    return l;
}

QLabel *hint(const QString &text, const QColor &color)
{
    auto *l = label(text, color, 11); l->setWordWrap(true); return l;
}

// An icon of the workspace set turned by `degrees` (chevron-down up is 180).
QIcon turned(const QString &name, const QColor &color, int degrees)
{
    const QPixmap pixmap = workspaceIcon(name, color).pixmap(32, 32);
    return QIcon(degrees ? pixmap.transformed(QTransform().rotate(degrees), Qt::SmoothTransformation) : pixmap);
}

// A warning line: the warning icon and the text, in warn.
QWidget *warningLine(const QString &text, const QColor &color, const char *name)
{
    auto *w = new QWidget; w->setObjectName(QByteArray(name) + "Line");
    auto *h = new QHBoxLayout(w); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(6);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("warning", color).pixmap(13, 13)); icon->setFixedSize(14, 16);
    auto *l = label(text, color, 12, false, name); l->setWordWrap(true);
    h->addWidget(icon, 0, Qt::AlignTop); h->addWidget(l, 1);
    return w;
}

// A line that ends in "…" when it does not fit.
QLabel *ellipsis(const QString &text, const QColor &color, int pixels)
{
    auto *l = new ElidingLabel(text);
    QFont f = l->font(); f.setPixelSize(pixels); l->setFont(f);
    QPalette pal = l->palette(); pal.setColor(QPalette::WindowText, color); l->setPalette(pal);
    l->setToolTip(text);
    return l;
}

QToolButton *iconButton(const QIcon &icon, const QString &tip, const char *name)
{
    auto *b = new QToolButton; b->setObjectName(name); b->setIcon(icon); b->setAutoRaise(true); b->setToolTip(tip); b->setAccessibleName(tip);
    b->setCursor(Qt::PointingHandCursor);
    return b;
}

QWidget *box(QLayout *layout)
{
    auto *b = new QWidget; layout->setContentsMargins(0, 0, 0, 0); b->setLayout(layout); return b;
}

QString workText(const QString &work)
{
    if (work == "clone") return tr("own clone");
    if (work == "same") return tr("same folder");
    if (work == "cloud") return tr("cloud sandbox");
    return tr("worktree per member");
}

QString approvalText(const QString &approval)
{
    return approval == "auto" ? tr("Run when the plan is valid") : tr("Review plan first");
}

// The prototype's routeMatch, as the server's route_match (N1): "everything"
// anywhere, or a rule that is just "*", always matches; an empty rule never
// does; else any run of three or more ASCII letters or digits (stop words out)
// that the goal contains, or its singular for a plural of five letters or more.
bool routeMatch(const QString &when, const QString &goal)
{
    static const QStringList stop{"label", "files", "file", "the", "and", "with", "when", "services", "src"};
    const QString w = when.toLower();
    if (w.contains("everything") || w.trimmed() == "*") return true;
    static const QRegularExpression wordRe(QStringLiteral("[a-z0-9]{3,}"));
    for (auto it = wordRe.globalMatch(w); it.hasNext();) {
        const QString word = it.next().captured();
        if (stop.contains(word)) continue;
        if (goal.contains(word) || (word.size() > 4 && word.endsWith('s') && goal.contains(word.chopped(1)))) return true;
    }
    return false;
}

// U36: a stage's kind and a preset's final acceptance are named by
// Teams::stageName, as the desktop's chips name them; colored here as the
// mock-up colors them.
QColor stageColor(const QString &kind, const Colors &c)
{
    if (kind == "review") return c.violet;
    if (kind == "test") return c.folder;
    if (kind == "command") return c.warn;
    if (kind == "implement") return c.text2;
    return c.accent;   // land and the acceptances
}

QString acceptKind(const Teams::Pipeline &p) { return p.accept.isEmpty() ? QStringLiteral("lead") : p.accept; }
QString folderName(const QString &folder) { return folder == "checkout" ? tr("Detached checkout") : tr("Task's folder · holds it"); }
QString outcomeName(const QString &outcome)
{
    if (outcome == "accepted") return tr("Accepted");
    if (outcome == "returned") return tr("Returned to the implementer");
    return tr("Held for the lead");
}

// The role a new review or test stage gets: the reviewer or tester when the
// catalog has one, else the first role that is not the lead.
QString defaultStageRole(const Teams::Catalog &catalog, const QString &kind)
{
    const QString wanted = kind == "test" ? QStringLiteral("tester") : QStringLiteral("reviewer");
    if (catalog.role(wanted)) return wanted;
    for (const auto &r : catalog.roles) if (r.id != "lead") return r.id;
    return {};
}

// The prototype's stage icons in a 10-12 px box: the role icons for review,
// test, implement and the leads; a bolt, a terminal and an arrow onto a line.
void drawStageIcon(QPainter &p, const QRectF &box, const QColor &color, const QString &kind)
{
    if (kind == "review") return drawRoleIcon(p, box, color, "reviewer");
    if (kind == "test") return drawRoleIcon(p, box, color, "tester");
    if (kind == "implement") return drawRoleIcon(p, box, color, "secretary");
    if (kind == "lead" || kind == "sub_lead") return drawRoleIcon(p, box, color, "lead");
    p.save();
    p.setRenderHint(QPainter::Antialiasing);
    QPen pen(color, 1.2); pen.setCapStyle(Qt::RoundCap); pen.setJoinStyle(Qt::RoundJoin); p.setPen(pen); p.setBrush(Qt::NoBrush);
    const double x = box.x(), y = box.y(), w = box.width(), h = box.height();
    const auto at = [&](double fx, double fy) { return QPointF(x + fx * w, y + fy * h); };
    QPainterPath path;
    if (kind == "auto") {
        path.moveTo(at(.58, .06)); path.lineTo(at(.2, .56)); path.lineTo(at(.5, .56)); path.lineTo(at(.42, .94)); path.lineTo(at(.8, .44)); path.lineTo(at(.5, .44));
        path.closeSubpath();
    } else if (kind == "command") {
        p.drawRoundedRect(QRectF(at(.06, .14), at(.94, .86)), 1.5, 1.5);
        path.moveTo(at(.24, .36)); path.lineTo(at(.42, .5)); path.lineTo(at(.24, .64)); p.drawLine(at(.5, .66), at(.74, .66));
    } else {   // land: an arrow down onto a line
        path.moveTo(at(.5, .08)); path.lineTo(at(.5, .64)); path.moveTo(at(.28, .42)); path.lineTo(at(.5, .64)); path.lineTo(at(.72, .42));
        p.drawLine(at(.14, .88), at(.86, .88));
    }
    p.drawPath(path);
    p.restore();
}

// A stage chip: the kind's icon and name on its tint; with no text, the round
// marker of a Try it step.
class StageChip : public QWidget {
public:
    StageChip(const QString &kind, const QString &text, const QColor &color) : m_kind(kind), m_text(text), m_color(color)
    {
        QFont f = font(); f.setPixelSize(11); f.setWeight(QFont::DemiBold); setFont(f);
        setObjectName(text.isEmpty() ? "teamPipelineStepMark" : "teamPipelineStageChip"); setProperty("kind", kind);
        if (!text.isEmpty()) setToolTip(text);
        setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
    }
    QSize sizeHint() const override { return m_text.isEmpty() ? QSize(26, 26) : QSize(fontMetrics().horizontalAdvance(m_text) + 28, 20); }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        if (m_text.isEmpty()) {
            p.setPen(QPen(alpha(m_color, .45), 1)); p.setBrush(alpha(m_color, .12)); p.drawEllipse(QRectF(rect()).adjusted(1, 1, -1, -1));
            drawStageIcon(p, QRectF(7, 7, 12, 12), m_color, m_kind);
            return;
        }
        const QRectF r = QRectF(rect()).adjusted(.5, 1.5, -.5, -1.5);
        p.setPen(Qt::NoPen); p.setBrush(alpha(m_color, .14)); p.drawRoundedRect(r, 5, 5);
        drawStageIcon(p, QRectF(6, (height() - 10) / 2.0, 10, 10), m_color, m_kind);
        p.setPen(m_color);
        p.drawText(QRect(20, 0, width() - 24, height()), Qt::AlignVCenter | Qt::AlignLeft, m_text);
    }
private:
    QString m_kind, m_text; QColor m_color;
};

// A preset's flow: its stages, then its final acceptance (none after a land
// stage, which decides itself); past `most` chips the rest is "+N".
QWidget *stageFlow(const Teams::Pipeline &p, const Colors &c, int most)
{
    auto *w = new QWidget; w->setObjectName("teamPipelineFlow");
    auto *h = new QHBoxLayout(w); h->setContentsMargins(0, 2, 0, 0); h->setSpacing(3);
    QStringList kinds; for (const auto &s : p.stages) kinds << s.kind;
    if (kinds.isEmpty() || kinds.last() != "land") kinds << acceptKind(p);
    for (int i = 0; i < kinds.size(); ++i) {
        if (i == most) { h->addWidget(label(QStringLiteral("+%1").arg(kinds.size() - most), c.muted, 11)); break; }
        if (i) h->addWidget(label(QStringLiteral("›"), c.dim, 11));
        h->addWidget(new StageChip(kinds[i], stageName(kinds[i]), stageColor(kinds[i], c)));
    }
    h->addStretch();
    return w;
}
}

TeamCatalogEditor::Route TeamCatalogEditor::route(const Teams::Catalog &catalog, const QString &goal)
{
    const QString lowered = goal.toLower();
    Route found;
    for (int i = 0; i < catalog.routing.size(); ++i) {
        const auto &rule = catalog.routing[i];
        if (!routeMatch(rule.when, lowered)) continue;
        const QString use = rule.use.isEmpty() ? rule.templateId : rule.use;
        if (use.startsWith('+')) { if (!found.added.contains(use.mid(1))) found.added << use.mid(1); continue; }
        if (const auto *t = catalog.templ(use)) {
            found.rule = i; found.templateId = t->id; found.approval = rule.approval.isEmpty() ? t->approval : rule.approval;
            return found;
        }
    }
    if (!catalog.templates.isEmpty()) { found.templateId = catalog.templates.first().id; found.approval = catalog.templates.first().approval; }
    return found;
}

// The role and template lists ask for the height of their rows: Settings >
// Teams lays the editor out at its size hint inside a scrolling page, where
// QListWidget's default hint (about five rows) cut the list short with the
// column empty below "+ Role" (owner, 2026-10-28). A short column still
// shrinks the list, which then scrolls.
class RowsList : public QListWidget {
public:
    int rowsHeight = 0;
    QSize sizeHint() const override
    {
        QSize size = QListWidget::sizeHint();
        if (rowsHeight > 0) size.setHeight(rowsHeight);
        return size;
    }
};

TeamCatalogEditor::TeamCatalogEditor(TeamController *teams, QWidget *parent) : QWidget(parent), m_teams(teams)
{
    setObjectName("teamCatalogEditor");
    setAttribute(Qt::WA_StyledBackground);
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(10);
    // Header (spec 11.2): title, meta and the actions of the mode.
    auto *header = new QHBoxLayout; header->setSpacing(8);
    m_title = new QLabel; m_title->setObjectName("teamCatalogTitle");
    m_meta = new QLabel; m_meta->setObjectName("teamCatalogStatus"); m_meta->setTextFormat(Qt::RichText);
    m_meta->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    header->addWidget(m_title); header->addWidget(m_meta, 1);
    m_openDesigner = button(tr("Open Team designer"), "teamCatalogAskDesigner"); m_openDesigner->hide();
    m_import = button(tr("Import…"), "teamCatalogImport"); m_import->setProperty("ghost", true);
    m_export = button(tr("Export…"), "teamCatalogExport"); m_export->setProperty("ghost", true);
    m_revert = button(tr("Revert"), "teamCatalogRevert");
    m_save = button(tr("Save catalog"), "teamCatalogSave", true);
    m_discard = button(tr("Discard draft"), "teamDesignerDiscard"); m_discard->setProperty("danger", true);
    m_apply = button(tr("Apply"), "teamDesignerApply", true);
    for (auto *b : {m_openDesigner, m_import, m_export, m_revert, m_save, m_discard, m_apply}) header->addWidget(b);
    outer->addLayout(header);
    // Banner: working designer, out-of-date draft, a result.
    m_banner = new QFrame; m_banner->setObjectName("teamCatalogBanner"); m_banner->hide();
    auto *bh = new QHBoxLayout(m_banner); bh->setContentsMargins(12, 8, 12, 8); bh->setSpacing(10);
    m_bannerIcon = new QLabel; m_bannerIcon->setFixedSize(18, 18);
    auto *bt = new QVBoxLayout; bt->setSpacing(2);
    m_bannerTitle = new QLabel; m_bannerTitle->setObjectName("teamCatalogBannerTitle");
    m_bannerText = new QLabel; m_bannerText->setWordWrap(true); m_bannerText->setObjectName("teamCatalogBannerText"); m_bannerText->setTextFormat(Qt::PlainText);
    bt->addWidget(m_bannerTitle); bt->addWidget(m_bannerText);
    m_bannerAction = button(QString(), "teamCatalogRedo"); m_bannerAction->hide();
    m_bannerDiscard = button(tr("Discard draft"), "teamCatalogBannerDiscard"); m_bannerDiscard->hide(); m_bannerDiscard->setProperty("danger", true);
    bh->addWidget(m_bannerIcon, 0, Qt::AlignTop); bh->addLayout(bt, 1); bh->addWidget(m_bannerAction); bh->addWidget(m_bannerDiscard);
    outer->addWidget(m_banner);
    // Segments with the diff legend on the right.
    auto *tabsRow = new QHBoxLayout;
    m_tabs = segmented({{"roles", tr("Roles")}, {"templates", tr("Templates")}, {"routing", tr("Routing")}, {"pipelines", tr("Pipelines")}, {"rules", tr("Planning rules")}},
                       "roles", "teamCatalogTabs", [this](const QString &v) {
                           setTab(v == "roles" ? Tab::Roles : v == "templates" ? Tab::Templates : v == "routing" ? Tab::Routing : v == "pipelines" ? Tab::Pipelines : Tab::Rules);
                       });
    m_legend = new QLabel; m_legend->setObjectName("teamCatalogLegend"); m_legend->setTextFormat(Qt::RichText);
    tabsRow->addWidget(m_tabs); tabsRow->addStretch(); tabsRow->addWidget(m_legend);
    outer->addLayout(tabsRow);
    // The list column and the editor, each scrolling on its own.
    auto *bodyRow = new QHBoxLayout; bodyRow->setSpacing(14);
    m_left = new QWidget; m_left->setObjectName("teamCatalogListColumn"); m_left->setFixedWidth(listWidth);
    auto *lv = new QVBoxLayout(m_left); lv->setContentsMargins(0, 0, 0, 0); lv->setSpacing(4);
    const auto makeList = [this](const char *name) {
        auto *list = new RowsList; list->setObjectName(name);
        list->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        list->setFrameShape(QFrame::NoFrame); list->setSpacing(1);
        // As tall as its rows up to the full column, "+ Role" right under
        // them; only a column shorter than the rows scrolls the list. A
        // Maximum policy held it at the widget's default hint (about five rows).
        list->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Preferred);
        list->setMinimumHeight(rowHeight);
        connect(list, &QListWidget::currentRowChanged, this, [this] { rebuildEditor(); });
        return list;
    };
    m_roles = makeList("teamCatalogRoles"); m_templates = makeList("teamCatalogTemplates"); m_pipelines = makeList("teamCatalogPipelines");
    m_add = button(tr("+ Role"), "teamCatalogAdd"); m_add->setProperty("ghost", true);
    lv->addWidget(m_roles, 1); lv->addWidget(m_templates, 1); lv->addWidget(m_pipelines, 1); lv->addWidget(m_add, 0, Qt::AlignLeft); lv->addStretch();
    connect(m_add, &QPushButton::clicked, this, [this] {
        if (m_tab == Tab::Roles) {
            Role r; r.name = tr("New role"); r.id = slug(r.name);
            for (int n = 2; m_catalog.role(r.id); ++n) r.id = slug(r.name) + QStringLiteral("-%1").arg(n);
            r.agent = "claude"; r.folder = "worktree"; r.canMessage = "lead"; r.color = swatches[m_catalog.roles.size() % swatches.size()];
            m_catalog.roles.append(r); changed(); rebuildLists(); m_roles->setCurrentRow(m_catalog.roles.size() - 1);
        } else if (m_tab == Tab::Pipelines) {
            // U36: a new preset starts with a review by the reviewer (or the
            // first role that is not the lead); the lead accepts.
            Pipeline p; p.name = tr("New pipeline"); p.id = slug(p.name);
            for (int n = 2; m_catalog.pipeline(p.id); ++n) p.id = slug(p.name) + QStringLiteral("-%1").arg(n);
            p.maxRounds = 3; p.folder = "hold"; p.accept = "lead";
            QString role = m_catalog.role("reviewer") ? QStringLiteral("reviewer") : QString();
            for (const auto &r : m_catalog.roles) if (role.isEmpty() && r.id != "lead") role = r.id;
            if (!role.isEmpty()) { PipelineStage stage; stage.kind = "review"; stage.role = role; p.stages.append(stage); }
            m_catalog.pipelines.append(p); changed(); rebuildLists(); m_pipelines->setCurrentRow(m_catalog.pipelines.size() - 1);
        } else {
            Template t; t.name = tr("New template"); t.id = slug(t.name);
            for (int n = 2; m_catalog.templ(t.id); ++n) t.id = slug(t.name) + QStringLiteral("-%1").arg(n);
            t.planning = "lead"; t.approval = "review"; t.work = "worktree";
            m_catalog.templates.append(t); changed(); rebuildLists(); m_templates->setCurrentRow(m_catalog.templates.size() - 1);
        }
    });
    m_body = new QScrollArea; m_body->setObjectName("teamCatalogEditorArea"); m_body->setWidgetResizable(true); m_body->setFrameShape(QFrame::NoFrame);
    m_body->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_body->viewport()->setAutoFillBackground(false);
    auto *content = new QWidget; content->setObjectName("teamCatalogBody");
    m_editorHost = new QVBoxLayout(content); m_editorHost->setContentsMargins(0, 0, 6, 0); m_editorHost->addStretch();
    m_body->setWidget(content);
    bodyRow->addWidget(m_left); bodyRow->addWidget(m_body, 1);
    outer->addLayout(bodyRow, 1);

    connect(m_openDesigner, &QPushButton::clicked, this, [this] { if (askDesigner) askDesigner(); });
    connect(m_revert, &QPushButton::clicked, this, [this] {
        if (confirm && !confirm(QStringLiteral("teamCatalogRevertConfirm"), tr("Revert catalog"),
                                counted(unsavedChanges(), tr("Drop %1 unsaved change?"), tr("Drop %1 unsaved changes?")), tr("Revert"))) return;
        load();
    });
    connect(m_save, &QPushButton::clicked, this, [this] {
        m_save->setEnabled(false); m_meta->setText(tr("Saving…"));
        QJsonObject input{{"catalog", m_catalog.json()}, {"version", m_catalog.version}};
        m_teams->call(QStringLiteral("catalog set"), input, [this](bool ok, const QJsonObject &result, const QString &error) {
            if (!ok) {
                m_meta->setText(QStringLiteral("⚠ ") + error.toHtmlEscaped()); m_save->setEnabled(true);
                // A newer version: read it, and the edits move onto it (rebase()).
                if (error.contains(QLatin1String("catalog changed"))) m_teams->refreshCatalog();
                return;
            }
            m_dirty = false;
            m_applied = tr("Saved as v%1").arg(result.value("version").toInteger());
            updateHeader();
            m_teams->refreshCatalog();
        });
    });
    connect(m_discard, &QPushButton::clicked, this, [this] { discardDraft(); });
    connect(m_bannerDiscard, &QPushButton::clicked, this, [this] { discardDraft(); });
    connect(m_apply, &QPushButton::clicked, this, &TeamCatalogEditor::applyKept);
    connect(m_bannerAction, &QPushButton::clicked, this, [this] { if (askRedo) askRedo(m_teams->catalog().version); });
    connect(m_export, &QPushButton::clicked, this, [this] {
        const auto path = QFileDialog::getSaveFileName(this, tr("Export team catalog"), QStringLiteral("zerus-team-catalog.json"), tr("JSON (*.json)"));
        if (path.isEmpty()) return;
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly) || file.write(QJsonDocument(m_catalog.json()).toJson()) < 0)
            m_meta->setText(tr("⚠ Could not write %1").arg(path).toHtmlEscaped());
        else m_meta->setText(tr("Exported to %1").arg(path).toHtmlEscaped());
    });
    connect(m_import, &QPushButton::clicked, this, [this] {
        const auto path = QFileDialog::getOpenFileName(this, tr("Import team catalog"), {}, tr("JSON (*.json)"));
        if (path.isEmpty()) return;
        const auto before = m_catalog.json();
        const qint64 version = m_catalog.version;
        ProcessRunner::post(this, [path] {
            QFile file(path);
            QString error;
            if (!file.open(QIODevice::ReadOnly)) return qMakePair(QJsonObject(), false);
            const auto data = JsonParse::object(file.readAll(), &error);
            return qMakePair(data, error.isEmpty());
        }, [this, path, before, version](const QPair<QJsonObject, bool> &parsed) {
            if (!parsed.second) { m_meta->setText(tr("⚠ %1 is not a team catalog").arg(path).toHtmlEscaped()); return; }
            if (m_catalog.version != version || m_catalog.json() != before) {
                m_meta->setText(tr("⚠ The catalog changed while importing; try again.")); return;
            }
            // An export holds the catalog itself; `catalog get` wraps it.
            m_catalog = parseCatalog(parsed.first.contains("catalog") ? parsed.first : QJsonObject{{"catalog", parsed.first}});
            m_catalog.version = version; m_catalog.proposals.clear();
            m_readOnly = false;
            changed(); rebuildLists(); rebuildEditor();
        });
    });
    connect(m_teams, &TeamController::catalogChanged, this, [this] { if (!m_dirty) load(); else rebase(); });
    setDark(true);
    if (m_teams->catalogReady()) load(); else m_teams->refreshCatalog();
}

void TeamCatalogEditor::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    const auto a = [](const QColor &color, double value) { return alpha(color, value).name(QColor::HexArgb); };
    setStyleSheet(teamSheet(c) + QStringLiteral(
        "QWidget#teamCatalogEditor{background:%11;}"
        "QScrollArea#teamCatalogEditorArea,QWidget#teamCatalogBody{background:transparent;}"
        "QLabel#teamCatalogTitle{color:%1;font-size:15px;font-weight:600;}"
        "QWidget#teamCatalogListColumn{border-right:1px solid %6;}"
        "QListWidget#teamCatalogRoles,QListWidget#teamCatalogTemplates,QListWidget#teamCatalogPipelines{background:transparent;border:none;outline:none;}"
        "QListWidget#teamCatalogRoles::item,QListWidget#teamCatalogTemplates::item,QListWidget#teamCatalogPipelines::item{border:1px solid transparent;border-radius:8px;margin-right:8px;}"
        "QListWidget#teamCatalogRoles::item:selected,QListWidget#teamCatalogTemplates::item:selected,QListWidget#teamCatalogPipelines::item:selected{background:%2;border-color:%3;}"
        "QListWidget#teamCatalogRoles::item:hover,QListWidget#teamCatalogTemplates::item:hover,QListWidget#teamCatalogPipelines::item:hover{background:%4;}"
        "QFrame#teamPipelineStageDetails{background:transparent;border:1px solid %6;border-radius:9px;}"
        "QFrame#teamPipelineTryBox{background:transparent;border:1px dashed %6;border-radius:10px;}"
        "QFrame#teamCatalogCard{background:%5;border:1px solid %6;border-radius:10px;}"
        "QFrame#teamProposalPlate{background:%7;border:1px dashed %8;border-radius:8px;}"
        "QFrame#teamPromptOrder{border:1px solid %6;border-radius:8px;background:transparent;}"
        "QFrame#teamPromptBlock{border:none;border-bottom:1px solid %6;background:transparent;}"
        "QFrame#teamRoutingTryBox{background:%5;border:1px solid %6;border-radius:10px;}"
        "QWidget[stepperBox=\"true\"]{border:1px solid %6;border-radius:7px;background:%9;}"
        "QPushButton[stepper=\"true\"]{border:none;border-radius:6px;padding:0;color:%1;}"
        "QPushButton[stepper=\"true\"]:disabled{color:%10;}"
        "QLabel#teamStepperValue{color:%1;font-size:12px;}"
        "QPushButton[swatch]{border-radius:5px;border:2px solid transparent;}"
        "QPushButton[roleIcon]{border-radius:6px;border:1px solid transparent;padding:2px;}"
        "QPushButton[roleIcon]:checked{background:%2;border-color:%3;}")
        .arg(c.fg.name(), a(c.accent, dark ? .10 : .08), a(c.accent, .45), a(c.fg, .04), c.cardFill.name(), c.border.name(),
             a(c.folder, .06), a(c.folder, .55), c.card.name(), c.dim.name(), c.bg.name()));
    m_import->setIcon(turned("send", c.text2, 180)); m_export->setIcon(turned("send", c.text2, 0));
    m_openDesigner->setIcon(workspaceIcon("team", c.text2));
    rebuildLists(); rebuildEditor(); updateHeader();
}

void TeamCatalogEditor::setDesignerButton(bool shown) { m_openDesigner->setProperty("wanted", shown); updateHeader(); }

bool TeamCatalogEditor::draftMode() const { return m_showDraft && !m_teams->catalog().proposals.isEmpty() && !m_dirty; }

const Teams::Proposal *TeamCatalogEditor::proposalOf(const QString &kind, const QString &id) const
{
    if (!draftMode()) return nullptr;
    const Teams::Proposal *found = nullptr;
    for (const auto &p : m_teams->catalog().proposals) if (p.kind == kind && p.target == id) found = &p;
    return found;
}

void TeamCatalogEditor::load()
{
    const auto &base = m_teams->catalog();
    m_catalog = base;
    m_catalog.proposals.clear();
    m_loaded = m_catalog;
    m_dirty = false;
    m_readOnly = draftMode();
    if (m_readOnly) {
        // The draft view: every proposal over the saved catalog; a removed or
        // rejected item keeps its saved form, so the list still shows it.
        for (const auto &p : base.proposals) {
            if (p.action == "remove" || p.state == "rejected") {
                if (p.action == "add" && p.state == "rejected") {
                    if (p.kind == "role" && !m_catalog.role(p.target)) m_catalog.roles.append(parseRole(p.value));
                    else if (p.kind == "template" && !m_catalog.templ(p.target)) m_catalog.templates.append(parseTemplate(p.value));
                    else if (p.kind == "rule") m_catalog.routing.append(parseRule(p.value));
                    else if (p.kind == "pipeline" && !m_catalog.pipeline(p.target)) m_catalog.pipelines.append(parsePipeline(p.value));
                }
                continue;
            }
            const auto put = [&](auto &items, const auto &parsed) {
                for (auto &item : items) if (item.id == p.target) { item = parsed; return; }
                items.append(parsed);
            };
            if (p.kind == "role") put(m_catalog.roles, parseRole(p.value));
            else if (p.kind == "template") put(m_catalog.templates, parseTemplate(p.value));
            else if (p.kind == "rule") put(m_catalog.routing, parseRule(p.value));
            else if (p.kind == "pipeline") put(m_catalog.pipelines, parsePipeline(p.value));
            else if (p.kind == "planning_rules") m_catalog.planningRules = p.text;
        }
    }
    rebuildLists(); rebuildEditor(); updateHeader();
}

namespace {
// Three-way, by id: the items edited, added or removed in mine since loaded go
// over now; the rest is now's.
template <typename T>
QList<T> rebased(const QList<T> &loaded, const QList<T> &mine, const QList<T> &now)
{
    const auto find = [](const QList<T> &items, const QString &id) -> qsizetype {
        for (qsizetype i = 0; i < items.size(); ++i) if (items[i].id == id) return i;
        return -1;
    };
    QList<T> out = now;
    for (const auto &old : loaded) {
        if (find(mine, old.id) >= 0) continue;
        if (const auto at = find(out, old.id); at >= 0) out.removeAt(at);
    }
    for (const auto &item : mine) {
        const auto old = find(loaded, item.id);
        if (old >= 0 && loaded[old].json() == item.json()) continue;
        if (const auto at = find(out, item.id); at >= 0) out[at] = item; else out.append(item);
    }
    return out;
}

QJsonArray rulesJson(const QList<Teams::Rule> &rules)
{
    QJsonArray out;
    for (const auto &rule : rules) out.append(rule.json());
    return out;
}
}

void TeamCatalogEditor::rebase()
{
    const auto &base = m_teams->catalog();
    if (base.version == m_loaded.version) { updateHeader(); return; }
    Teams::Catalog next = base;
    next.proposals.clear();
    next.roles = rebased(m_loaded.roles, m_catalog.roles, base.roles);
    next.templates = rebased(m_loaded.templates, m_catalog.templates, base.templates);
    next.pipelines = rebased(m_loaded.pipelines, m_catalog.pipelines, base.pipelines);
    if (rulesJson(m_catalog.routing) != rulesJson(m_loaded.routing)) next.routing = m_catalog.routing;
    if (m_catalog.planningRules != m_loaded.planningRules) next.planningRules = m_catalog.planningRules;
    m_loaded = base;
    m_loaded.proposals.clear();
    m_catalog = next;
    // Edits the newer catalog already has leave nothing to save.
    if (!unsavedChanges()) { load(); return; }
    m_readOnly = false;
    rebuildLists(); rebuildEditor(); updateHeader();
}

void TeamCatalogEditor::changed()
{
    m_dirty = true;
    m_applied.clear();
    rebuildLists();   // the edited item's dot
    updateHeader();
}

// U15: decisions go through the controller, whatever the designer session's
// state; a refusal stays on its proposal's plate.
void TeamCatalogEditor::decide(const QString &proposal, const QString &decision)
{
    m_teams->decideProposal(proposal, decision);
}

void TeamCatalogEditor::discardDraft()
{
    if (confirm && !confirm(QStringLiteral("teamCatalogDiscardConfirm"), tr("Discard draft"),
                            tr("Drop every proposal in the designer's draft? The catalog stays as it is."), tr("Discard draft"))) return;
    m_teams->discardDraft();   // a refusal shows in the banner
}

int TeamCatalogEditor::unsavedChanges() const
{
    if (!m_dirty) return 0;
    const auto &base = m_teams->catalog();
    int n = 0;
    const auto count = [&n](const QMap<QString, QJsonObject> &a, const QMap<QString, QJsonObject> &b) {
        for (auto it = a.cbegin(); it != a.cend(); ++it) n += b.value(it.key()) != it.value();
        for (auto it = b.cbegin(); it != b.cend(); ++it) n += !a.contains(it.key());
    };
    QMap<QString, QJsonObject> mine, theirs;
    for (const auto &r : m_catalog.roles) mine.insert("r" + r.id, r.json());
    for (const auto &r : base.roles) theirs.insert("r" + r.id, r.json());
    for (const auto &t : m_catalog.templates) mine.insert("t" + t.id, t.json());
    for (const auto &t : base.templates) theirs.insert("t" + t.id, t.json());
    for (const auto &r : m_catalog.routing) mine.insert("u" + r.id, r.json());
    for (const auto &r : base.routing) theirs.insert("u" + r.id, r.json());
    for (const auto &p : m_catalog.pipelines) mine.insert("p" + p.id, p.json());
    for (const auto &p : base.pipelines) theirs.insert("p" + p.id, p.json());
    count(mine, theirs);
    // A new order of the same rules is one change too.
    QStringList order, baseOrder;
    for (const auto &r : m_catalog.routing) order << r.id;
    for (const auto &r : base.routing) baseOrder << r.id;
    if (n == 0 && order != baseOrder) n = 1;
    // U28: the planning rules are one change.
    n += m_catalog.planningRules != base.planningRules;
    return qMax(1, n);
}

void TeamCatalogEditor::updateHeader()
{
    updatePlanningRulesReset();
    const Colors c = colorsFor(m_dark);
    const auto &base = m_teams->catalog();
    const bool draft = draftMode();
    int kept = 0, undecided = 0, total = base.proposals.size();
    QMap<QString, int> perKind;
    for (const auto &p : base.proposals) { kept += p.state == "kept"; undecided += p.state == "proposed"; ++perKind[p.kind]; }
    const bool stale = draft && base.draftBase != base.version;
    m_title->setText(draft ? tr("Draft catalog") : tr("Team catalog"));
    const QString version = base.builtin ? tr("built-in") : QStringLiteral("v%1").arg(base.version);
    if (draft) m_meta->setText(QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.muted.name(),
        tr("based on v%1 · %2 of %3 kept · %4 undecided").arg(base.draftBase).arg(kept).arg(total).arg(undecided)));
    else if (m_dirty) m_meta->setText(QStringLiteral("<span style='color:%1;'>%2 · </span><span style='color:%3;'>● %4</span>")
        .arg(c.muted.name(), version, c.warn.name(), counted(unsavedChanges(), tr("%1 unsaved change"), tr("%1 unsaved changes"))));
    else m_meta->setText(QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.muted.name(), base.builtin ? version : version + tr(" · saved")));
    for (auto *b : {m_import, m_export, m_revert, m_save}) b->setVisible(!draft);
    m_openDesigner->setVisible(m_openDesigner->property("wanted").toBool() && !draft);
    m_revert->setEnabled(m_dirty); m_save->setEnabled(m_dirty);
    // Reset waits for Save or Revert: an edit after the tab was built disables it too.
    for (auto *reset : findChildren<QPushButton *>(QStringLiteral("teamPlanningRulesReset"))) {
        reset->setEnabled(!m_dirty && base.planningRulesKnown);
        reset->setToolTip(m_dirty ? tr("Save or revert your edits first") : tr("Replace the rules with Zerus's built-in ones and save the catalog"));
    }
    m_discard->setVisible(draft); m_apply->setVisible(draft);
    m_apply->setText(QStringLiteral("✓ ") + counted(kept, tr("Apply %1 change"), tr("Apply %1 changes")));
    m_apply->setEnabled(draft && kept > 0 && !stale);
    m_apply->setToolTip(stale ? tr("The catalog changed after this draft; applying would overwrite those edits.")
                              : undecided ? tr("Undecided changes stay in the draft") : QString());
    // Counters per tab while a draft shows.
    const QStringList kinds{"role", "template", "rule", "pipeline", "planning_rules"};
    const QStringList names{tr("Roles"), tr("Templates"), tr("Routing"), tr("Pipelines"), tr("Planning rules")};
    const auto buttons = m_tabs->findChildren<QPushButton *>();
    for (int i = 0; i < buttons.size() && i < kinds.size(); ++i)
        buttons[i]->setText(draft && perKind.value(kinds[i]) ? QStringLiteral("%1   %2").arg(names[i]).arg(perKind.value(kinds[i])) : names[i]);
    m_legend->setVisible(draft);
    const auto legendMark = [](const QString &text, const QColor &color) {
        return QStringLiteral("<span style='color:%1;background:%2;font-size:10px;font-weight:700;'>&nbsp;%3&nbsp;</span>")
            .arg(color.name(), alpha(color, .12).name(QColor::HexArgb), text);
    };
    m_legend->setText(legendMark(QStringLiteral("NEW"), c.accent) + QStringLiteral(" &nbsp;") + legendMark(QStringLiteral("CHANGED"), c.folder)
                      + QStringLiteral(" &nbsp;") + legendMark(QStringLiteral("REMOVED"), c.danger));
    // One banner at a time: a refused Apply or Discard, out of date, working
    // designer, then a result.
    QString title, text; QColor tone; bool redo = false; QString icon = QStringLiteral("team");
    if (draft && !m_teams->draftError().isEmpty()) {
        title = tr("The draft was not changed"); text = m_teams->draftError(); tone = c.danger; icon = QStringLiteral("warning");
    } else if (stale) {
        title = tr("Draft is out of date — Apply is off");
        text = tr("The catalog was saved as v%1 after this draft started on v%2.").arg(base.version).arg(base.draftBase);
        tone = c.warn; redo = true; icon = QStringLiteral("warning");
    } else if (!draft && m_designerWorking) {
        title = tr("The designer is working");
        text = tr("Its proposals will appear here as a draft. Saving manual edits now makes that draft out of date.");
        tone = c.accent;
    } else if (!m_applied.isEmpty()) { title = m_applied; tone = c.accent; icon = QStringLiteral("check-circle"); }
    m_banner->setVisible(!title.isEmpty());
    m_bannerTitle->setText(title); m_bannerText->setText(text); m_bannerText->setVisible(!text.isEmpty());
    if (tone.isValid()) m_bannerIcon->setPixmap(workspaceIcon(icon, tone).pixmap(18, 18));
    m_banner->setStyleSheet(QStringLiteral("QFrame#teamCatalogBanner{background:%1;border:1px solid %2;border-radius:9px;}"
                                           "QLabel#teamCatalogBannerTitle{color:%3;font-weight:600;font-size:13px;} QLabel#teamCatalogBannerText{color:%4;font-size:12px;}")
                                .arg(alpha(tone, .08).name(QColor::HexArgb), alpha(tone, .45).name(QColor::HexArgb), c.fg.name(), c.text2.name()));
    m_bannerAction->setVisible(redo && askRedo && (!canRedo || canRedo()));
    m_bannerAction->setText(tr("Ask designer to redo"));
    m_bannerAction->setToolTip(tr("Ask the designer to propose again on v%1").arg(base.version));
    m_bannerDiscard->setVisible(redo);
}

void TeamCatalogEditor::setTab(Tab tab)
{
    m_tab = tab;
    for (auto *b : m_tabs->findChildren<QPushButton *>()) {
        const auto v = b->property("segment").toString();
        b->setChecked((v == "roles" && tab == Tab::Roles) || (v == "templates" && tab == Tab::Templates) || (v == "routing" && tab == Tab::Routing)
                      || (v == "pipelines" && tab == Tab::Pipelines) || (v == "rules" && tab == Tab::Rules));
    }
    rebuildLists(); rebuildEditor();
}

void TeamCatalogEditor::selectTemplate(const QString &id)
{
    setTab(Tab::Templates);
    for (int i = 0; i < m_templates->count(); ++i) if (m_templates->item(i)->data(Qt::UserRole).toString() == id) m_templates->setCurrentRow(i);
}

QWidget *TeamCatalogEditor::listRow(const QString &kind, int index, const Proposal *proposal, bool dirty)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QWidget;
    auto *h = new QHBoxLayout(w); h->setContentsMargins(10, 4, 8, 4); h->setSpacing(6);
    if (kind == "role") {
        const Role &r = m_catalog.roles[index];
        w->setProperty("catalogItem", r.id);
        h->addWidget(chipOf(r, m_dark));
        h->addStretch();
        // The computers it is bound to, when not any.
        const QStringList machines = !r.machines.isEmpty() ? r.machines : r.computer.isEmpty() ? QStringList() : QStringList{r.computer};
        if (!machines.isEmpty() && !machines.contains("any")) { auto *m = ellipsis(machines.join(", "), c.muted, 11); m->setMaximumWidth(70); h->addWidget(m); }
        if (proposal) h->addWidget(mark(*proposal, c));
        if (dirty) h->addWidget(label(QStringLiteral("●"), c.warn, 9));
        auto *tile = new QLabel; tile->setPixmap(agentLogo(r.agent.isEmpty() ? QStringLiteral("claude") : r.agent, c.fg).pixmap(14, 14));
        tile->setToolTip(agentNames.value(r.agent, r.agent)); h->addWidget(tile);
    } else if (kind == "pipeline") {
        // U36: name, rounds, folder and whose default it is, then its flow.
        auto *v = new QVBoxLayout; v->setSpacing(2);
        if (index < 0) {
            w->setProperty("catalogItem", QStringLiteral("none"));
            auto *name = ellipsis(tr("None"), c.fg, 13); QFont f = name->font(); f.setWeight(QFont::Medium); name->setFont(f);
            v->addWidget(name); v->addWidget(ellipsis(tr("no stages: the lead decides, as today"), c.muted, 11));
            h->addLayout(v, 1);
            return w;
        }
        const Pipeline &p = m_catalog.pipelines[index];
        w->setProperty("catalogItem", p.id);
        auto *name = ellipsis(p.name, c.fg, 13); QFont f = name->font(); f.setWeight(QFont::Medium); name->setFont(f);
        QStringList meta;
        meta << (p.stages.isEmpty() ? tr("today's flow") : counted(p.rounds(), tr("%1 round"), tr("%1 rounds")));
        bool tests = false; for (const auto &s : p.stages) tests = tests || s.kind == "test";
        if (tests) meta << (p.folder == "checkout" ? tr("detached checkout") : tr("task's folder"));
        if (const auto users = pipelineUsers(p.id); !users.isEmpty())
            meta << (users.size() <= 2 ? tr("default of %1").arg(users.join(", ")) : tr("default of %1 templates and roles").arg(users.size()));
        auto *metaLine = ellipsis(meta.join(QStringLiteral(" · ")), c.muted, 11); metaLine->setObjectName("teamPipelineMeta");
        auto *nameRow = row(6); nameRow->addWidget(name, 1);
        if (proposal) nameRow->addWidget(mark(*proposal, c));
        if (dirty) nameRow->addWidget(label(QStringLiteral("●"), c.warn, 9));
        v->addLayout(nameRow); v->addWidget(metaLine);
        v->addWidget(stageFlow(p, c, p.stages.size() + 1 > 3 ? 2 : 3));
        h->addLayout(v, 1);
    } else {
        const Template &t = m_catalog.templates[index];
        w->setProperty("catalogItem", t.id);
        auto *v = new QVBoxLayout; v->setSpacing(1);
        auto *name = ellipsis(t.name, c.fg, 13); QFont f = name->font(); f.setWeight(QFont::Medium); name->setFont(f);
        int members = 0, optional = 0;
        for (const auto &seat : t.roster) (seat.optional ? optional : members) += seat.count;
        const QString count = optional ? tr("%1+%2 members").arg(members).arg(optional) : counted(members, tr("%1 member"), tr("%1 members"));
        const QString tasks = t.tasks.isEmpty() ? tr("lead plans") : counted(int(t.tasks.size()), tr("%1 task"), tr("%1 tasks"));
        auto *meta = ellipsis(QStringLiteral("%1 · %2 · %3").arg(count, tasks, workText(t.work)), c.muted, 11);
        meta->setObjectName("teamTemplateMeta");
        v->addWidget(name); v->addWidget(meta);
        h->addLayout(v, 1);
        if (proposal) h->addWidget(mark(*proposal, c), 0, Qt::AlignTop);
        if (dirty) h->addWidget(label(QStringLiteral("●"), c.warn, 9), 0, Qt::AlignTop);
    }
    if (proposal && proposal->state == "rejected") { auto *fx = new QGraphicsOpacityEffect(w); fx->setOpacity(.55); w->setGraphicsEffect(fx); }
    return w;
}

void TeamCatalogEditor::rebuildLists()
{
    const auto &base = m_teams->catalog();
    const auto fill = [&](QListWidget *list, const QString &kind) {
        const QString current = list->currentItem() ? list->currentItem()->data(Qt::UserRole).toString() : QString();
        const QSignalBlocker block(list);
        list->clear();
        const int size = kind == "role" ? int(m_catalog.roles.size()) : int(m_catalog.templates.size());
        for (int i = 0; i < size; ++i) {
            const QString id = kind == "role" ? m_catalog.roles[i].id : m_catalog.templates[i].id;
            const QJsonObject mine = kind == "role" ? m_catalog.roles[i].json() : m_catalog.templates[i].json();
            QJsonObject saved;
            if (kind == "role") { if (const auto *r = base.role(id)) saved = r->json(); }
            else if (const auto *t = base.templ(id)) saved = t->json();
            const bool dirty = !m_readOnly && saved != mine;
            auto *item = new QListWidgetItem; item->setData(Qt::UserRole, id);
            item->setSizeHint(QSize(listWidth - 12, kind == "role" ? rowHeight + 4 : templateRowHeight));
            list->addItem(item);
            list->setItemWidget(item, listRow(kind, i, proposalOf(kind, id), dirty));
        }
        int row = 0, height = 2 * list->frameWidth();
        for (int i = 0; i < list->count(); ++i) {
            if (list->item(i)->data(Qt::UserRole).toString() == current) row = i;
            height += list->item(i)->sizeHint().height() + 2 * list->spacing();
        }
        list->setCurrentRow(list->count() ? row : -1);
        list->setMaximumHeight(qMax(rowHeight, height + 2));
        if (auto *rows = dynamic_cast<RowsList *>(list)) { rows->rowsHeight = qMax(rowHeight, height + 2); rows->updateGeometry(); }
    };
    fill(m_roles, "role"); fill(m_templates, "template");
    // U36: the presets, then None (a task without a pipeline).
    {
        // None alone (before the catalog loads) is no choice: the first preset is.
        const QString current = m_pipelines->count() > 1 && m_pipelines->currentItem() ? m_pipelines->currentItem()->data(Qt::UserRole).toString() : QString();
        const QSignalBlocker block(m_pipelines);
        m_pipelines->clear();
        for (int i = 0; i <= m_catalog.pipelines.size(); ++i) {
            const bool none = i == m_catalog.pipelines.size();
            const QString id = none ? QStringLiteral("none") : m_catalog.pipelines[i].id;
            bool dirty = false;
            if (!none && !m_readOnly) { const auto *saved = base.pipeline(id); dirty = !saved || saved->json() != m_catalog.pipelines[i].json(); }
            auto *item = new QListWidgetItem; item->setData(Qt::UserRole, id);
            item->setSizeHint(QSize(listWidth - 12, none ? templateRowHeight : pipelineRowHeight));
            m_pipelines->addItem(item);
            m_pipelines->setItemWidget(item, listRow(QStringLiteral("pipeline"), none ? -1 : i, none ? nullptr : proposalOf("pipeline", id), dirty));
        }
        int row = 0, height = 2 * m_pipelines->frameWidth();
        for (int i = 0; i < m_pipelines->count(); ++i) {
            if (m_pipelines->item(i)->data(Qt::UserRole).toString() == current) row = i;
            height += m_pipelines->item(i)->sizeHint().height() + 2 * m_pipelines->spacing();
        }
        m_pipelines->setCurrentRow(row);
        m_pipelines->setMaximumHeight(qMax(rowHeight, height + 2));
        if (auto *rows = dynamic_cast<RowsList *>(m_pipelines)) { rows->rowsHeight = qMax(rowHeight, height + 2); rows->updateGeometry(); }
    }
    m_roles->setVisible(m_tab == Tab::Roles); m_templates->setVisible(m_tab == Tab::Templates); m_pipelines->setVisible(m_tab == Tab::Pipelines);
    m_left->setVisible(m_tab == Tab::Roles || m_tab == Tab::Templates || m_tab == Tab::Pipelines);
    m_add->setText(m_tab == Tab::Roles ? tr("+ Role") : m_tab == Tab::Pipelines ? tr("+ Pipeline") : tr("+ Template"));
    m_add->setEnabled(m_tab != Tab::Pipelines || m_catalog.pipelines.size() < 20);
    m_add->setVisible(!m_readOnly);
}

void TeamCatalogEditor::rebuildEditor()
{
    QWidget *editor = nullptr;
    auto *list = m_tab == Tab::Roles ? m_roles : m_tab == Tab::Templates ? m_templates : m_tab == Tab::Pipelines ? m_pipelines : nullptr;
    const QString id = list && list->currentItem() ? list->currentItem()->data(Qt::UserRole).toString() : QString();
    if (m_tab == Tab::Roles) { for (int i = 0; i < m_catalog.roles.size(); ++i) if (m_catalog.roles[i].id == id) editor = roleEditor(i); }
    else if (m_tab == Tab::Templates) { for (int i = 0; i < m_catalog.templates.size(); ++i) if (m_catalog.templates[i].id == id) editor = templateEditor(i); }
    else if (m_tab == Tab::Rules) editor = planningRulesEditor();
    else if (m_tab == Tab::Pipelines) {
        if (id == "none") editor = pipelineEditor(-1);
        for (int i = 0; i < m_catalog.pipelines.size(); ++i) if (m_catalog.pipelines[i].id == id) editor = pipelineEditor(i);
    }
    else editor = routingEditor();
    if (!editor) editor = label(tr("Nothing selected"), colorsFor(m_dark).muted, 12);
    const int scroll = m_body->verticalScrollBar() ? m_body->verticalScrollBar()->value() : 0;
    if (m_editorCard) { m_editorHost->removeWidget(m_editorCard); retire(m_editorCard); }   // a click inside it may be running
    m_editorCard = editor;
    // A checked segment is bold: each keeps the room its bold text needs.
    for (auto *segments : editor->findChildren<QWidget *>())
        if (segments->property("seg").toBool())
            for (auto *b : segments->findChildren<QPushButton *>()) {
                QFont bold = b->font(); bold.setWeight(QFont::DemiBold);
                b->setMinimumWidth(QFontMetrics(bold).horizontalAdvance(b->text()) + 28);
            }
    m_editorHost->insertWidget(0, editor);
    // A rebuild of the same item keeps its place.
    const QString key = QStringLiteral("%1/%2").arg(int(m_tab)).arg(id);
    if (property("editorKey").toString() == key) m_body->verticalScrollBar()->setValue(scroll);
    setProperty("editorKey", key);
}

QWidget *TeamCatalogEditor::proposalPlate(const Proposal &p)
{
    const Colors c = colorsFor(m_dark);
    auto *plate = new QFrame; plate->setObjectName("teamProposalPlate");
    auto *v = new QVBoxLayout(plate); v->setContentsMargins(12, 8, 12, 9); v->setSpacing(4);
    auto *head = row(8);
    head->addWidget(mark(p, c));
    auto *title = label(p.title, p.state == "rejected" ? c.dim : c.fg, 13, true); title->setWordWrap(true);
    if (p.state == "rejected") { QFont f = title->font(); f.setStrikeOut(true); title->setFont(f); }
    head->addWidget(title, 1);
    if (p.state == "proposed") {
        auto *reject = button(tr("Reject"), "teamProposalReject"); reject->setProperty("ghost", true);
        auto *keep = button(tr("Keep"), "teamProposalKeep");
        connect(reject, &QPushButton::clicked, this, [this, id = p.id] { decide(id, "reject"); });
        connect(keep, &QPushButton::clicked, this, [this, id = p.id] { decide(id, "keep"); });
        head->addWidget(reject); head->addWidget(keep);
    } else {
        head->addWidget(label(p.state == "kept" ? tr("Kept") : tr("Rejected"), p.state == "kept" ? c.accent : c.danger, 12));
        auto *undo = button(tr("Undo"), "teamProposalUndo"); undo->setProperty("ghost", true);
        connect(undo, &QPushButton::clicked, this, [this, id = p.id] { decide(id, "undo"); });
        head->addWidget(undo);
    }
    v->addLayout(head);
    if (const auto error = m_teams->proposalError(p.id); !error.isEmpty()) v->addWidget(refusal(error, c, "teamProposalError"));
    if (!p.reason.isEmpty()) v->addWidget(hint(p.reason, c.text2));
    return plate;
}

QWidget *TeamCatalogEditor::roleEditor(int index)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QFrame; w->setObjectName("teamCatalogCard");
    auto *outer = new QVBoxLayout(w); outer->setContentsMargins(16, 14, 16, 16); outer->setSpacing(12);
    Role &r = m_catalog.roles[index];
    const Role *orig = m_readOnly ? nullptr : m_teams->catalog().role(r.id);
    const auto edited = [&](const auto &mine, const auto &theirs) { return orig && mine != theirs; };
    const auto set = [this](const std::function<void()> &apply) { return [this, apply] { apply(); changed(); }; };
    // Header: badge, about, Duplicate, delete (never the lead's).
    auto *head = row(8);
    head->addWidget(chipOf(r, m_dark));
    head->addWidget(ellipsis(r.about.isEmpty() ? r.prompt.section('\n', 0, 0) : r.about, c.muted, 13), 1);
    auto *duplicate = button(tr("Duplicate"), "teamRoleDuplicate"); duplicate->setProperty("ghost", true); duplicate->setIcon(workspaceIcon("copy", c.text2));
    auto *remove = iconButton(workspaceIcon("trash", c.danger), tr("Delete role"), "teamRoleDelete");
    remove->setVisible(r.id != "lead");
    head->addWidget(duplicate); head->addWidget(remove);
    duplicate->setEnabled(!m_readOnly); remove->setEnabled(!m_readOnly);
    outer->addLayout(head);
    if (const auto *p = proposalOf("role", r.id)) outer->addWidget(proposalPlate(*p));
    auto *form = new QWidget; form->setObjectName("teamRoleEditor");
    auto *grid = new QGridLayout(form); grid->setContentsMargins(0, 0, 0, 0); grid->setHorizontalSpacing(14); grid->setVerticalSpacing(10);
    grid->setColumnMinimumWidth(0, labelWidth); grid->setColumnStretch(1, 1);
    int at = 0;
    const auto add = [&](const QString &text, bool changedField, QWidget *field) { grid->addWidget(fieldLabel(text, changedField, c), at, 0, Qt::AlignTop | Qt::AlignRight); grid->addWidget(field, at++, 1); };
    auto *name = new QLineEdit(r.name); name->setObjectName("teamRoleName");
    connect(name, &QLineEdit::editingFinished, this, [this, index, name] {
        const auto text = name->text().trimmed(); if (text.isEmpty() || text == m_catalog.roles[index].name) return;
        m_catalog.roles[index].name = text; changed();
    });
    add(tr("Name"), edited(r.name, orig ? orig->name : QString()), name);
    auto *about = new QLineEdit(r.about); about->setObjectName("teamRoleAbout"); about->setPlaceholderText(tr("What this role does, in a few words"));
    connect(about, &QLineEdit::editingFinished, this, [this, index, about] {
        if (about->text().trimmed() == m_catalog.roles[index].about) return;
        m_catalog.roles[index].about = about->text().trimmed(); changed();
    });
    add(tr("About"), edited(r.about, orig ? orig->about : QString()), about);
    // Icon / color: the palette's 16 colors, then its 10 icons.
    auto *look = new QVBoxLayout; look->setSpacing(6);
    auto *colorBox = new QWidget; auto *colors = new FlowLayout(6); colorBox->setLayout(colors);
    const QString chosenColor = r.color.isEmpty() ? Teams::roleColorKey(r.name) : r.color;
    for (const auto &swatch : swatches) {
        auto *b = new QPushButton; b->setObjectName("teamRoleSwatch"); b->setProperty("swatch", swatch); b->setFixedSize(20, 20); b->setCursor(Qt::PointingHandCursor);
        b->setToolTip(swatch); b->setAccessibleName(swatch);
        const QColor fill = swatchColor(swatch, m_dark);
        b->setStyleSheet(QStringLiteral("QPushButton{background:%1;border:2px solid %2;border-radius:5px;}")
                             .arg(fill.name(), swatch == chosenColor || swatchColor(chosenColor, m_dark) == fill ? c.fg.name() : QStringLiteral("transparent")));
        connect(b, &QPushButton::clicked, this, [this, index, swatch] { m_catalog.roles[index].color = swatch; changed(); rebuildEditor(); });
        colors->addWidget(b);
    }
    look->addWidget(colorBox);
    auto *icons = row(4);
    const QString chosenIcon = iconOf(r);
    for (const auto &icon : roleIcons) {
        auto *b = new QPushButton; b->setObjectName("teamRoleIcon"); b->setProperty("roleIcon", icon); b->setCheckable(true);
        b->setChecked(icon == chosenIcon); b->setFixedSize(26, 24); b->setIcon(roleIcon(icon, icon == chosenIcon ? colorOf(r, m_dark) : c.text2));
        b->setToolTip(icon); b->setCursor(Qt::PointingHandCursor);
        connect(b, &QPushButton::clicked, this, [this, index, icon] { m_catalog.roles[index].icon = QStringLiteral("r_") + icon; changed(); rebuildEditor(); });
        icons->addWidget(b);
    }
    icons->addStretch();
    look->addLayout(icons);
    add(tr("Icon / color"), edited(r.color + r.icon, orig ? orig->color + orig->icon : QString()), box(look));
    // Specialization: chips with ×, a field that adds one on Enter.
    auto *tagsBox = new QWidget; auto *tags = new FlowLayout(6); tagsBox->setLayout(tags);
    for (int i = 0; i < r.tags.size(); ++i) {
        auto *chip = button(r.tags[i] + QStringLiteral("  ×"), "teamRoleTag"); chip->setProperty("chip", true);
        chip->setToolTip(tr("Remove %1").arg(r.tags[i]));
        connect(chip, &QPushButton::clicked, this, [this, index, i] { m_catalog.roles[index].tags.removeAt(i); changed(); rebuildEditor(); });
        tags->addWidget(chip);
    }
    auto *skill = new SkillEdit; skill->setObjectName("teamRoleTags"); skill->setPlaceholderText(tr("+ skill, Enter")); skill->setFixedWidth(170);
    connect(skill, &QLineEdit::returnPressed, this, [this, index, skill] {
        const auto text = skill->text().trimmed();
        if (text.isEmpty() || m_catalog.roles[index].tags.contains(text)) return;
        m_catalog.roles[index].tags << text; changed(); rebuildEditor();
        if (auto *again = m_editorCard ? m_editorCard->findChild<QLineEdit *>("teamRoleTags") : nullptr) again->setFocus();
    });
    skill->backspaceEmpty = [this, index] {
        auto &list = m_catalog.roles[index].tags; if (list.isEmpty()) return;
        list.removeLast(); changed(); rebuildEditor();
        if (auto *again = m_editorCard ? m_editorCard->findChild<QLineEdit *>("teamRoleTags") : nullptr) again->setFocus();
    };
    tags->addWidget(skill);
    add(tr("Specialization"), edited(r.tags, orig ? orig->tags : QStringList()), tagsBox);
    // Default agent: agent, account, model, effort.
    auto *agentRow = row(6);
    auto *logo = new QLabel; logo->setPixmap(agentLogo(r.agent.isEmpty() ? QStringLiteral("claude") : r.agent, c.fg).pixmap(16, 16)); logo->setFixedWidth(20);
    QList<std::pair<QString, QString>> agentItems; for (const auto &a : agents) agentItems.append({a, agentNames.value(a)});
    auto *agent = choice(agentItems, r.agent, "teamRoleAgent");
    auto *account = new QComboBox; account->setObjectName("teamRoleAccount"); fillAccounts(account, r);
    m_accountChoice = account; m_accountRole = index;
    // Native account models replace the offline suggestions when they arrive.
    auto *model = new QComboBox; model->setObjectName("teamRoleModel"); model->setEditable(true); model->setInsertPolicy(QComboBox::NoInsert);
    fillModels(model, r); model->lineEdit()->setPlaceholderText(tr("default model"));
    m_modelChoice = model; m_modelRole = index; requestModels(r);
    auto *effort = choice({{"", "—"}, {"low", "low"}, {"medium", "medium"}, {"high", "high"}, {"xhigh", "xhigh"}}, r.effort, "teamRoleEffort");
    for (auto *combo : {agent, account, model, effort}) combo->setMinimumWidth(60);
    agentRow->addWidget(logo); agentRow->addWidget(agent, 11); agentRow->addWidget(account, 10); agentRow->addWidget(model, 11); agentRow->addWidget(effort, 7);
    // Another agent: its first model and the default account (spec 11.3).
    connect(agent, &QComboBox::activated, this, [this, index, agent] {
        auto &role = m_catalog.roles[index]; const auto value = agent->currentData().toString(); if (value == role.agent) return;
        role.agent = value; role.account.clear(); role.model = Teams::knownModels(value).value(0);
        changed(); rebuildEditor();
    });
    connect(account, &QComboBox::activated, this, [this, index, account, model] {
        auto &role = m_catalog.roles[index]; role.account = account->currentData().toString(); changed();
        fillModels(model, role); requestModels(role);
    });
    const auto modelChanged = [this, index, model] {
        const auto text = model->currentText().trimmed(); if (text == m_catalog.roles[index].model) return;
        m_catalog.roles[index].model = text; changed();
    };
    connect(model->lineEdit(), &QLineEdit::editingFinished, this, modelChanged);
    connect(model, &QComboBox::activated, this, modelChanged);
    connect(effort, &QComboBox::activated, this, set([this, index, effort] { m_catalog.roles[index].effort = effort->currentData().toString(); }));
    add(tr("Default agent"), orig && (r.agent != orig->agent || r.account != orig->account || r.model != orig->model || r.effort != orig->effort), box(agentRow));
    // U45: the models a member of this role may switch to before a task.
    auto *modelsBox = new QVBoxLayout; modelsBox->setSpacing(4);
    auto *choicesBox = new QWidget; auto *choices = new FlowLayout(6); choicesBox->setLayout(choices);
    for (int i = 0; i < r.models.size(); ++i) {
        auto *chip = button(r.models[i].text() + QStringLiteral("  ×"), "teamRoleModelChoice"); chip->setProperty("chip", true);
        chip->setToolTip(tr("Remove %1").arg(r.models[i].text()));
        connect(chip, &QPushButton::clicked, this, [this, index, i] { m_catalog.roles[index].models.removeAt(i); changed(); rebuildEditor(); });
        choices->addWidget(chip);
    }
    auto *choiceModel = new QComboBox; choiceModel->setObjectName("teamRoleModelsAdd"); choiceModel->setEditable(true); choiceModel->setInsertPolicy(QComboBox::NoInsert);
    for (int i = 0; i < model->count(); ++i) if (!model->itemText(i).isEmpty()) choiceModel->addItem(model->itemText(i));
    choiceModel->setCurrentIndex(-1); choiceModel->lineEdit()->setPlaceholderText(tr("+ model")); choiceModel->setMinimumWidth(150);
    auto *choiceEffort = choice({{"", "—"}, {"low", "low"}, {"medium", "medium"}, {"high", "high"}, {"xhigh", "xhigh"}}, QString(), "teamRoleModelsEffort");
    auto *addChoice = button(tr("Add"), "teamRoleModelsAddButton"); addChoice->setProperty("ghost", true);
    const auto addModel = [this, index, choiceModel, choiceEffort] {
        const Teams::ModelChoice next{choiceModel->currentText().trimmed(), choiceEffort->currentData().toString()};
        auto &list = m_catalog.roles[index].models;
        if (next.model.isEmpty() || list.contains(next) || list.size() >= 20) return;
        list.append(next); changed(); rebuildEditor();
    };
    connect(addChoice, &QPushButton::clicked, this, addModel);
    connect(choiceModel->lineEdit(), &QLineEdit::returnPressed, this, addModel);
    auto *addRow = row(6); addRow->addWidget(choiceModel); addRow->addWidget(choiceEffort); addRow->addWidget(addChoice); addRow->addStretch();
    modelsBox->addWidget(choicesBox);
    modelsBox->addLayout(addRow);
    auto *modelsHint = hint(r.models.isEmpty() ? tr("Only the default model. A task that names other models goes to a member that allows one")
                                               : tr("Before a task that names one of these, Zerus switches the member to it and waits for the switch"), c.muted);
    modelsHint->setObjectName("teamRoleModelsHint");
    modelsBox->addWidget(modelsHint);
    choicesBox->setVisible(!r.models.isEmpty());
    add(tr("Models"), edited(r.models, orig ? orig->models : QList<Teams::ModelChoice>()), box(modelsBox));
    // U46: what Zerus does to the member's context before each task.
    auto *contextBox = new QVBoxLayout; contextBox->setSpacing(4);
    auto *contextRow = row(0);
    const QString context = r.contextBeforeTask.isEmpty() ? QStringLiteral("none") : r.contextBeforeTask;
    contextRow->addWidget(segmented({{"none", tr("Keep")}, {"compact", tr("Compact")}, {"clear", tr("Clear")}}, context, "teamRoleContext",
                                    [this, index](const QString &value) {
                                        auto &role = m_catalog.roles[index];
                                        // The default stays unwritten unless the role already had the field.
                                        const QString next = value == "none" && !role.raw.contains(QLatin1String("context_before_task")) ? QString() : value;
                                        if (next == role.contextBeforeTask) return;
                                        role.contextBeforeTask = next; changed(); rebuildEditor();
                                    }));
    contextRow->addStretch();
    contextBox->addLayout(contextRow);
    auto *contextHint = hint(context == "compact" ? tr("Before each task the conversation is compacted; the task goes once that is confirmed")
                             : context == "clear" ? tr("Before each task the conversation is cleared; the task text names everything it needs")
                                                  : tr("Each task continues the member's conversation"), c.muted);
    contextHint->setObjectName("teamRoleContextHint");
    contextBox->addWidget(contextHint);
    add(tr("Context before task"), edited(r.contextBeforeTask, orig ? orig->contextBeforeTask : QString()), box(contextBox));
    // Computer: chips with ×, "Any computer" when none; "+ computer" adds one.
    auto *computerBox = new QVBoxLayout; computerBox->setSpacing(4);
    auto *chips = new QWidget; auto *flow = new FlowLayout(6); chips->setLayout(flow);
    QStringList machines = r.machines; if (machines.isEmpty() && !r.computer.isEmpty()) machines << r.computer;
    machines.removeAll(QStringLiteral("any"));
    const auto online = [this](const QString &alias) {
        for (const auto &m : m_computers) if (m.alias == alias) return m.online;
        return true;
    };
    const auto labelOf = [this](const QString &alias) {
        for (const auto &m : m_computers) if (m.alias == alias) return m.label.isEmpty() ? alias : m.label;
        return alias;
    };
    const auto setMachines = [this, index](const QStringList &list) {
        auto &role = m_catalog.roles[index];
        const QString first = list.value(0);
        role.machines = list;
        if (first != role.computer) { role.computer = first; role.account.clear(); }
        changed(); rebuildEditor();
    };
    if (machines.isEmpty()) {
        auto *any = new QLabel(tr("Any computer")); any->setObjectName("teamRoleAnyComputer");
        any->setStyleSheet(QStringLiteral("color:%1;border:1px dashed %2;border-radius:6px;padding:3px 9px;font-size:12px;").arg(c.muted.name(), c.border.name()));
        flow->addWidget(any);
    }
    for (const auto &alias : machines) {
        auto *chip = button(labelOf(alias) + QStringLiteral("  ×"), "teamRoleMachine"); chip->setProperty("chip", true); chip->setProperty("machine", alias);
        if (!online(alias)) { QFont f = chip->font(); f.setStrikeOut(true); chip->setFont(f); chip->setToolTip(tr("%1 is offline").arg(labelOf(alias))); }
        connect(chip, &QPushButton::clicked, this, [setMachines, machines, alias] { auto list = machines; list.removeAll(alias); setMachines(list); });
        flow->addWidget(chip);
    }
    auto *computer = new QComboBox; computer->setObjectName("teamRoleComputer");
    computer->addItem(tr("+ computer"), QString()); computer->addItem(tr("Any computer"), QStringLiteral("any"));
    QStringList aliases; for (const auto &m : m_computers) if (!m.alias.isEmpty()) aliases << m.alias;
    for (const auto &alias : machines) if (!aliases.contains(alias)) aliases << alias;
    for (const auto &alias : aliases) computer->addItem(online(alias) ? labelOf(alias) : tr("%1 (offline)").arg(labelOf(alias)), alias);
    connect(computer, &QComboBox::activated, this, [setMachines, machines, computer] {
        const QString value = computer->currentData().toString(); if (value.isEmpty()) return;
        if (value == "any") { setMachines({}); return; }
        if (machines.contains(value)) { computer->setCurrentIndex(0); return; }
        setMachines(machines + QStringList{value});
    });
    flow->addWidget(computer);
    computerBox->addWidget(chips);
    computerBox->addWidget(hint(machines.isEmpty() ? tr("The lead picks a free computer with this agent signed in") : tr("Only these computers; the first free one wins"), c.muted));
    add(tr("Computer"), edited(machines, orig ? (orig->machines.isEmpty() && !orig->computer.isEmpty() ? QStringList{orig->computer} : orig->machines) : QStringList()), box(computerBox));
    // Permissions (C3/B5): a launch mode the members take, or a profile.
    const QString mode = r.permissions == "bypass" ? QStringLiteral("bypass") : r.permissions == "auto" ? QStringLiteral("auto")
                         : r.permissionsProfile == "read" ? QStringLiteral("read_only") : r.permissionsProfile == "write" ? QStringLiteral("ask")
                         : QStringLiteral("account");
    auto *permissionsBox = new QVBoxLayout; permissionsBox->setSpacing(4);
    auto *permissions = choice({{"ask", tr("Ask every time")}, {"auto", tr("Auto (workspace)")}, {"account", tr("Account’s mode")},
                                {"read_only", tr("Read only")}, {"bypass", tr("Bypass (dangerous)")}}, mode, "teamRolePermissions");
    permissions->setFixedWidth(260);
    connect(permissions, &QComboBox::activated, this, [this, index, permissions] {
        auto &role = m_catalog.roles[index]; const auto value = permissions->currentData().toString();
        role.permissions = value == "auto" || value == "bypass" ? value : QString();
        role.permissionsProfile = value == "read_only" ? QStringLiteral("read") : value == "ask" ? QStringLiteral("write") : QString();
        changed(); rebuildEditor();
    });
    permissionsBox->addWidget(permissions);
    if (mode == "bypass") { auto *note = hint(tr("Runs every tool without asking"), c.danger); note->setObjectName("teamRoleBypassNote"); permissionsBox->addWidget(note); }
    add(tr("Permissions"), orig && (r.permissions != orig->permissions || r.permissionsProfile != orig->permissionsProfile), box(permissionsBox));
    auto *folder = row(0);
    folder->addWidget(segmented({{"worktree", tr("Own worktree")}, {"shared", tr("Shared folder")}, {"read_only", tr("Read only")}}, r.folder.isEmpty() ? QStringLiteral("worktree") : r.folder,
                                "teamRoleFolder", [this, index](const QString &value) { m_catalog.roles[index].folder = value; changed(); }));
    folder->addStretch();
    add(tr("Folder policy"), edited(r.folder, orig ? orig->folder : QString()), box(folder));
    // Can message: lead, the lead and selected roles, or the whole team.
    const QString message = !r.message.isEmpty() ? r.message : r.canMessage.isEmpty() ? QStringLiteral("lead") : r.canMessage;
    auto *messageBox = new QVBoxLayout; messageBox->setSpacing(6);
    auto *messageRow = row(0);
    messageRow->addWidget(segmented({{"lead", tr("Lead only")}, {"some", tr("Lead + selected roles")}, {"team", tr("Whole team")}}, message,
                                    "teamRoleMessage", [this, index](const QString &value) {
                                        auto &role = m_catalog.roles[index];
                                        role.message = value; role.canMessage = value == "team" ? QStringLiteral("team") : QStringLiteral("lead");
                                        changed(); rebuildEditor();
                                    }));
    messageRow->addStretch();
    messageBox->addLayout(messageRow);
    if (message == "some") {
        auto *toBox = new QWidget; auto *to = new FlowLayout(6); toBox->setLayout(to);
        for (const auto &id : r.messageTo) {
            const Role *other = m_catalog.role(id);
            auto *chip = button((other ? other->name : id) + QStringLiteral("  ×"), "teamRoleMessageTo"); chip->setProperty("chip", true);
            connect(chip, &QPushButton::clicked, this, [this, index, id] { m_catalog.roles[index].messageTo.removeAll(id); changed(); rebuildEditor(); });
            to->addWidget(chip);
        }
        auto *more = new QComboBox; more->setObjectName("teamRoleMessageAdd"); more->addItem(tr("+ role"), QString());
        for (const auto &other : m_catalog.roles) if (other.id != r.id && other.id != "lead" && !r.messageTo.contains(other.id)) more->addItem(other.name, other.id);
        connect(more, &QComboBox::activated, this, [this, index, more] {
            const auto id = more->currentData().toString(); if (id.isEmpty()) return;
            m_catalog.roles[index].messageTo << id; changed(); rebuildEditor();
        });
        to->addWidget(more);
        messageBox->addWidget(toBox);
    }
    add(tr("Can message"), orig && (message != (!orig->message.isEmpty() ? orig->message : orig->canMessage.isEmpty() ? QStringLiteral("lead") : orig->canMessage)
                                     || r.messageTo != orig->messageTo), box(messageBox));
    auto *joins = new QLineEdit(r.joins); joins->setObjectName("teamRoleJoins");
    joins->setPlaceholderText(tr("file globs, e.g. services/payments/** — empty: only when a template asks"));
    connect(joins, &QLineEdit::editingFinished, this, [this, index, joins] {
        if (joins->text().trimmed() == m_catalog.roles[index].joins) return;
        m_catalog.roles[index].joins = joins->text().trimmed(); changed();
    });
    add(tr("Joins when"), edited(r.joins, orig ? orig->joins : QString()), joins);
    // Dangerous actions: five switches; the older single switch allowed push, deploy and delete.
    QJsonObject danger = r.danger;
    if (danger.isEmpty() && r.dangerous) danger = {{"push", true}, {"deploy", true}, {"del", true}};
    auto *dangerBox = new QVBoxLayout; dangerBox->setSpacing(4);
    auto *checks = new QGridLayout; checks->setHorizontalSpacing(24); checks->setVerticalSpacing(4);
    int allowed = 0;
    for (int i = 0; i < dangers.size(); ++i) {
        const auto &[key, text] = dangers[i];
        auto *check = new QCheckBox(tr(text)); check->setObjectName("teamRoleDanger"); check->setProperty("danger", key);
        check->setChecked(danger.value(key).toBool()); allowed += check->isChecked();
        connect(check, &QCheckBox::toggled, this, [this, index, key = key, danger](bool on) {
            auto &role = m_catalog.roles[index];
            QJsonObject next = danger;
            for (const auto &[k, t] : dangers) if (!next.contains(k)) next.insert(k, false);
            next.insert(key, on);
            role.danger = next;
            role.dangerous = std::any_of(dangers.cbegin(), dangers.cend(), [&](const auto &d) { return next.value(d.first).toBool(); });
            changed(); rebuildEditor();
        });
        checks->addWidget(check, i / 2, i % 2);
    }
    checks->setColumnStretch(2, 1);
    dangerBox->addLayout(checks);
    auto *summary = hint(tr("%1 of 5 allowed · only inside tasks you approved; everything else asks first.").arg(allowed), c.muted);
    summary->setObjectName("teamRoleDangerSummary");
    dangerBox->addWidget(summary);
    add(tr("Dangerous actions"), orig && (r.danger != orig->danger || r.dangerous != orig->dangerous), box(dangerBox));
    add(tr("Prompt order"), orig && (r.prompt != orig->prompt || r.promptOrder != orig->promptOrder || r.promptOff != orig->promptOff
                                     || r.protocolExtra != orig->protocolExtra || r.taskTemplate != orig->taskTemplate || r.notes != orig->notes),
        promptOrder(index));
    // Used in: the templates that seat it; a click opens one.
    auto *usedBox = new QWidget; auto *used = new FlowLayout(6); usedBox->setLayout(used);
    for (const auto &t : m_catalog.templates) for (const auto &seat : t.roster) if (seat.role == r.id) {
        auto *chip = button(QStringLiteral("%1 × %2").arg(t.name).arg(seat.count) + (seat.optional ? tr(" · if needed") : QString()), "teamRoleUsedIn");
        chip->setProperty("chip", true);
        connect(chip, &QPushButton::clicked, this, [this, id = t.id] { selectTemplate(id); });
        used->addWidget(chip);
    }
    if (!used->count()) used->addWidget(label(tr("No template uses this role"), c.dim, 12));
    add(tr("Used in"), false, usedBox);
    // The form shows the draft read only; Keep or Reject decides it.
    if (m_readOnly) form->setEnabled(false);
    outer->addWidget(form);
    connect(duplicate, &QPushButton::clicked, this, [this, index] {
        Role copy = m_catalog.roles[index]; copy.name += tr(" copy"); copy.id = slug(copy.name); copy.raw.remove("id");
        for (int n = 2; m_catalog.role(copy.id); ++n) copy.id = slug(copy.name) + QStringLiteral("-%1").arg(n);
        m_catalog.roles.insert(index + 1, copy); changed(); rebuildLists(); m_roles->setCurrentRow(index + 1);
    });
    connect(remove, &QToolButton::clicked, this, [this, index] {
        const QString id = m_catalog.roles[index].id;
        QStringList losing;
        for (const auto &t : m_catalog.templates) if (std::any_of(t.roster.cbegin(), t.roster.cend(), [&](const Seat &s) { return s.role == id; })) losing << t.name;
        // #538 N9: the presets whose stages the role takes lose them too; say
        // which, and which are left with no stage at all.
        QStringList presets;
        for (const auto &p : m_catalog.pipelines) {
            const auto taken = std::count_if(p.stages.cbegin(), p.stages.cend(), [&](const PipelineStage &s) { return s.role == id; });
            if (taken) presets << (taken == p.stages.size() ? tr("%1 (left with no stage)").arg(p.name) : p.name);
        }
        QString text = losing.isEmpty() ? tr("Delete the role %1 from the catalog?").arg(m_catalog.roles[index].name)
                                        : tr("Delete the role %1? These templates lose it: %2.").arg(m_catalog.roles[index].name, losing.join(", "));
        if (!presets.isEmpty()) text += QLatin1Char(' ') + tr("These pipelines lose its stages: %1.").arg(presets.join("; "));
        text += QLatin1Char(' ') + tr("Running teams keep their members.");
        if (confirm && !confirm(QStringLiteral("teamRoleDeleteConfirm"), tr("Delete role"), text, tr("Delete"))) return;
        for (auto &t : m_catalog.templates) {
            t.roster.erase(std::remove_if(t.roster.begin(), t.roster.end(), [&](const Seat &s) { return s.role == id; }), t.roster.end());
            for (auto &task : t.tasks) if (task.role == id) task.role = QStringLiteral("lead");
        }
        m_catalog.routing.erase(std::remove_if(m_catalog.routing.begin(), m_catalog.routing.end(), [&](const Rule &rule) { return rule.use == "+" + id; }), m_catalog.routing.end());
        for (auto &other : m_catalog.roles) other.messageTo.removeAll(id);
        // U36: a preset's stages taken by the role go with it.
        for (auto &p : m_catalog.pipelines)
            p.stages.erase(std::remove_if(p.stages.begin(), p.stages.end(), [&](const PipelineStage &s) { return s.role == id; }), p.stages.end());
        m_catalog.roles.removeAt(index); changed(); rebuildLists(); rebuildEditor();
    });
    return w;
}

// Prompt order (spec 11.3): the blocks a member reads, top to bottom; each can
// be switched off and moved; notes are the owner's own blocks.
QWidget *TeamCatalogEditor::promptOrder(int index)
{
    const Colors c = colorsFor(m_dark);
    const Role &r = m_catalog.roles[index];
    QStringList order = r.promptOrder;
    if (order.isEmpty()) order = fixedBlocks + r.notes.keys();
    for (const auto &key : fixedBlocks) if (!order.contains(key)) order << key;
    for (const auto &key : r.notes.keys()) if (!order.contains(key)) order << key;
    auto *frame = new QFrame; frame->setObjectName("teamPromptOrder");
    auto *v = new QVBoxLayout(frame); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(0);
    // Any change writes the whole order, so what is shown is what is sent.
    const auto commit = [this, index, order](const std::function<void(Role &, QStringList &)> &apply, bool rebuild) {
        auto &role = m_catalog.roles[index]; QStringList next = order;
        apply(role, next);
        role.promptOrder = next;
        // C1: the task block shows the default template, so the order sends it.
        if (role.taskTemplate.isEmpty()) role.taskTemplate = defaultTaskTemplate;
        changed(); if (rebuild) rebuildEditor();
    };
    for (int i = 0; i < order.size(); ++i) {
        const QString key = order[i];
        const bool off = r.promptOff.contains(key);
        auto *block = new QFrame; block->setObjectName("teamPromptBlock"); block->setProperty("block", key);
        auto *bv = new QVBoxLayout(block); bv->setContentsMargins(12, 9, 12, 10); bv->setSpacing(6);
        auto *head = row(6);
        QString title = key == "rules" ? tr("Project rules") : key == "role" ? tr("Role · %1").arg(r.name) : key == "protocol" ? tr("Team protocol")
                        : key == "task" ? tr("Task (per attempt)") : tr("Note");
        auto *caption = label(QStringLiteral("%1 · %2").arg(i + 1).arg(title.toUpper()), c.muted, 10, true); caption->setObjectName("teamPromptTitle");
        head->addWidget(caption, 1);
        auto *include = new QCheckBox(tr("include")); include->setObjectName("teamPromptInclude"); include->setChecked(!off);
        connect(include, &QCheckBox::toggled, this, [commit, key](bool on) {
            commit([key, on](Role &role, QStringList &) { role.promptOff.removeAll(key); if (!on) role.promptOff << key; }, true);
        });
        auto *up = iconButton(turned("chevron-down", c.text2, 180), tr("Move up"), "teamPromptUp");
        auto *down = iconButton(turned("chevron-down", c.text2, 0), tr("Move down"), "teamPromptDown");
        up->setEnabled(i > 0); down->setEnabled(i + 1 < order.size());
        connect(up, &QToolButton::clicked, this, [commit, i] { commit([i](Role &, QStringList &list) { list.swapItemsAt(i, i - 1); }, true); });
        connect(down, &QToolButton::clicked, this, [commit, i] { commit([i](Role &, QStringList &list) { list.swapItemsAt(i, i + 1); }, true); });
        head->addWidget(include); head->addWidget(up); head->addWidget(down);
        if (!fixedBlocks.contains(key)) {
            auto *del = iconButton(workspaceIcon("trash", c.muted), tr("Remove block"), "teamPromptDelete");
            connect(del, &QToolButton::clicked, this, [commit, key] {
                commit([key](Role &role, QStringList &list) { list.removeAll(key); role.notes.remove(key); role.promptOff.removeAll(key); }, true);
            });
            head->addWidget(del);
        }
        bv->addLayout(head);
        auto *body = new QWidget; auto *bodyLayout = new QVBoxLayout(body); bodyLayout->setContentsMargins(0, 0, 0, 0); bodyLayout->setSpacing(4);
        if (key == "rules") {
            const QString rules = projectRules ? projectRules().trimmed() : QString();
            auto *text = label(rules.isEmpty() ? tr("The selected session's project has no rules.") : rules, rules.isEmpty() ? c.dim : c.fg, 12);
            text->setWordWrap(true); text->setObjectName("teamPromptRules");
            bodyLayout->addWidget(text);
            bodyLayout->addWidget(hint(tr("Edit the rules in a session’s side panel"), c.muted));
        } else if (key == "role") {
            auto *prompt = area(r.prompt, "teamRolePrompt", 3);
            connect(prompt, &QPlainTextEdit::textChanged, this, [this, index, prompt] { m_catalog.roles[index].prompt = prompt->toPlainText(); changed(); });
            bodyLayout->addWidget(prompt);
        } else if (key == "protocol") {
            auto *protocol = area(r.protocolExtra, "teamRoleProtocol", 3);
            protocol->setPlaceholderText(tr("Extra lines for this role on top of the template’s Lead protocol (optional)"));
            connect(protocol, &QPlainTextEdit::textChanged, this, [this, index, protocol, order] {
                auto &role = m_catalog.roles[index]; role.protocolExtra = protocol->toPlainText();
                if (role.promptOrder.isEmpty()) role.promptOrder = order;
                if (role.taskTemplate.isEmpty()) role.taskTemplate = defaultTaskTemplate;
                changed();
            });
            bodyLayout->addWidget(protocol);
        } else if (key == "task") {
            auto *task = area(r.taskTemplate.isEmpty() ? defaultTaskTemplate : r.taskTemplate, "teamRoleTaskTemplate", 3);
            connect(task, &QPlainTextEdit::textChanged, this, [this, index, task, order] {
                auto &role = m_catalog.roles[index]; role.taskTemplate = task->toPlainText();
                if (role.promptOrder.isEmpty()) role.promptOrder = order;
                changed();
            });
            bodyLayout->addWidget(task);
            bodyLayout->addWidget(hint(tr("{id} {title} {description} {criteria} are filled per attempt"), c.muted));
        } else {
            auto *note = area(r.notes.value(key), "teamPromptNote", 2);
            note->setPlaceholderText(tr("A note every member of this role reads"));
            connect(note, &QPlainTextEdit::textChanged, this, [this, index, note, key] { m_catalog.roles[index].notes.insert(key, note->toPlainText()); changed(); });
            bodyLayout->addWidget(note);
        }
        // A block switched off is pale and skipped.
        if (off) { auto *fx = new QGraphicsOpacityEffect(body); fx->setOpacity(.45); body->setGraphicsEffect(fx); }
        bv->addWidget(body);
        v->addWidget(block);
    }
    auto *foot = row(10); foot->setContentsMargins(12, 8, 12, 8);
    auto *addBlock = button(tr("+ Block"), "teamPromptAddBlock"); addBlock->setProperty("ghost", true);
    connect(addBlock, &QPushButton::clicked, this, [commit] {
        commit([](Role &role, QStringList &list) {
            QString key; int n = 1;
            do key = QStringLiteral("note%1").arg(n++); while (list.contains(key) || role.notes.contains(key));
            role.notes.insert(key, QString()); list << key;
        }, true);
    });
    foot->addWidget(addBlock);
    foot->addWidget(hint(tr("Blocks are sent top to bottom; switched-off blocks are skipped."), c.muted), 1);
    v->addLayout(foot);
    return frame;
}

QWidget *TeamCatalogEditor::templateEditor(int index)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QFrame; w->setObjectName("teamCatalogCard");
    auto *outer = new QVBoxLayout(w); outer->setContentsMargins(16, 14, 16, 16); outer->setSpacing(12);
    Template &t = m_catalog.templates[index];
    // Header: name, which rule uses it, Duplicate, Delete.
    auto *head = row(8);
    head->addWidget(label(t.name, c.fg, 15, true));
    QStringList rules;
    for (const auto &rule : m_catalog.routing) if (rule.templateId == t.id && !rule.use.startsWith('+')) rules << QStringLiteral("“%1”").arg(rule.when);
    auto *usedBy = ellipsis(rules.isEmpty() ? tr("· manual pick only") : tr("· used by routing %1").arg(rules.join(", ")), c.muted, 13);
    usedBy->setObjectName("teamTemplateUsedBy");
    head->addWidget(usedBy, 1);
    auto *duplicate = button(tr("Duplicate"), "teamTemplateDuplicate"); duplicate->setProperty("ghost", true); duplicate->setIcon(workspaceIcon("copy", c.text2));
    auto *remove = button(tr("Delete"), "teamTemplateDelete"); remove->setProperty("danger", true); remove->setIcon(workspaceIcon("trash", c.danger));
    duplicate->setEnabled(!m_readOnly); remove->setEnabled(!m_readOnly);
    head->addWidget(duplicate); head->addWidget(remove);
    outer->addLayout(head);
    if (const auto *p = proposalOf("template", t.id)) outer->addWidget(proposalPlate(*p));
    auto *form = new QWidget; form->setObjectName("teamTemplateEditor");
    auto *grid = new QGridLayout(form); grid->setContentsMargins(0, 0, 0, 0); grid->setHorizontalSpacing(14); grid->setVerticalSpacing(10);
    grid->setColumnMinimumWidth(0, labelWidth); grid->setColumnStretch(1, 1);
    int at = 0;
    const auto add = [&](QWidget *labelWidget, QWidget *field) { grid->addWidget(labelWidget, at, 0, Qt::AlignTop | Qt::AlignRight); grid->addWidget(field, at++, 1); };
    const auto addText = [&](const QString &text, QWidget *field) { add(fieldLabel(text, false, c), field); };
    // A folding block's label: a chevron and the name.
    const auto foldLabel = [&](const QString &text, const QString &key, const char *name) {
        auto *b = new QPushButton(text); b->setObjectName(name);
        b->setIcon(turned("chevron-down", c.muted, folded(key) ? -90 : 0)); b->setIconSize(QSize(10, 10));
        b->setProperty("ghost", true); b->setFixedWidth(labelWidth); b->setCursor(Qt::PointingHandCursor);
        b->setLayoutDirection(Qt::LeftToRight);
        b->setStyleSheet(QStringLiteral("QPushButton{color:%1;text-align:right;padding:4px 0;border:none;font-size:12px;}").arg(c.label.name()));
        b->setAccessibleName(text); b->setProperty("expanded", !folded(key));
        connect(b, &QPushButton::clicked, this, [this, key] { setFolded(key, !folded(key)); rebuildEditor(); });
        return b;
    };
    auto *name = new QLineEdit(t.name); name->setObjectName("teamTemplateName");
    connect(name, &QLineEdit::editingFinished, this, [this, index, name] {
        const auto text = name->text().trimmed(); if (text.isEmpty() || text == m_catalog.templates[index].name) return;
        m_catalog.templates[index].name = text; changed(); rebuildEditor();
    });
    addText(tr("Name"), name);
    auto *description = new QLineEdit(t.description); description->setObjectName("teamTemplateDescription"); description->setPlaceholderText(tr("When to use this template"));
    connect(description, &QLineEdit::editingFinished, this, [this, index, description] {
        if (description->text().trimmed() == m_catalog.templates[index].description) return;
        m_catalog.templates[index].description = description->text().trimmed(); changed();
    });
    addText(tr("Description"), description);
    auto *protocol = area(t.leadProtocol, "teamTemplateProtocol", 2, 6); protocol->setPlaceholderText(tr("Extra instructions for the coordinator of this kind of team"));
    connect(protocol, &QPlainTextEdit::textChanged, this, [this, index, protocol] { m_catalog.templates[index].leadProtocol = protocol->toPlainText(); changed(); });
    addText(tr("Lead protocol"), protocol);
    // Roster: the lead, then a count and "only if needed" per catalog role.
    int members = 0, optional = 0, roles = 0;
    for (const auto &seat : t.roster) { (seat.optional ? optional : members) += seat.count; roles += seat.count > 0; }
    const QString rosterSummary = (optional ? tr("%1 members + %2 only if needed").arg(members).arg(optional) : counted(members, tr("%1 member"), tr("%1 members")))
                                  + QStringLiteral(" · ") + counted(roles, tr("%1 role"), tr("%1 roles"));
    auto *rosterBox = new QWidget; rosterBox->setObjectName("teamTemplateRoster");
    auto *rv = new QVBoxLayout(rosterBox); rv->setContentsMargins(0, 0, 0, 0); rv->setSpacing(6);
    if (folded("roster")) {
        auto *summaryRow = new QWidget; auto *flow = new FlowLayout(6); summaryRow->setLayout(flow);
        auto *summary = label(rosterSummary, c.text2, 12); summary->setObjectName("teamTemplateRosterSummary"); flow->addWidget(summary);
        if (const Role *lead = m_catalog.role("lead")) flow->addWidget(chipOf(*lead, m_dark));
        for (const auto &seat : t.roster) if (const Role *role = m_catalog.role(seat.role); role && seat.count > 0)
            flow->addWidget(chipOf(*role, m_dark, (seat.count > 1 ? QStringLiteral(" ×%1").arg(seat.count) : QString()) + (seat.optional ? QStringLiteral("?") : QString()), seat.optional));
        rv->addWidget(summaryRow);
    } else {
        auto *seats = new QGridLayout; seats->setHorizontalSpacing(12); seats->setVerticalSpacing(5);
        int line = 0;
        if (const Role *lead = m_catalog.role("lead")) {
            seats->addWidget(chipOf(*lead, m_dark, {}, false, 170), line, 0);
            seats->addWidget(label(tr("1 · the session that starts the team (coordinator)"), c.muted, 12), line++, 1, 1, 3);
        }
        const bool hideUnused = Settings::value(QStringLiteral("ui/catalogHideUnusedRoles"), false).toBool();
        int unused = 0;
        for (const auto &role : m_catalog.roles) {
            if (role.id == "lead") continue;
            int seat = -1;
            for (int i = 0; i < t.roster.size(); ++i) if (t.roster[i].role == role.id) seat = i;
            const int count = seat >= 0 ? t.roster[seat].count : 0;
            if (count == 0) { ++unused; if (hideUnused) continue; }
            auto *chip = chipOf(role, m_dark, {}, false, 170);
            if (count == 0) { auto *fx = new QGraphicsOpacityEffect(chip); fx->setOpacity(.5); chip->setGraphicsEffect(fx); }
            auto *stepper = new Stepper(count, 0, 9, "teamTemplateSeatCount", [this, index, id = role.id](int n) {
                auto &list = m_catalog.templates[index].roster;
                int at = -1; for (int i = 0; i < list.size(); ++i) if (list[i].role == id) at = i;
                if (n == 0 && at >= 0) list.removeAt(at);
                else if (n > 0 && at < 0) list.append({id, n, false, {}});
                else if (at >= 0) list[at].count = n;
                changed(); rebuildEditor();
            });
            stepper->setProperty("role", role.id);
            auto *only = new QCheckBox(tr("only if needed")); only->setObjectName("teamTemplateSeatOptional"); only->setProperty("role", role.id);
            only->setChecked(seat >= 0 && t.roster[seat].optional); only->setEnabled(count > 0);
            only->setToolTip(tr("The lead adds this member only when the plan needs it"));
            connect(only, &QCheckBox::toggled, this, [this, index, id = role.id](bool on) {
                for (auto &s : m_catalog.templates[index].roster) if (s.role == id) s.optional = on;
                changed(); rebuildEditor();
            });
            auto *agentInfo = row(6);
            auto *logo = new QLabel; logo->setPixmap(agentLogo(role.agent.isEmpty() ? QStringLiteral("claude") : role.agent, c.fg).pixmap(14, 14));
            QStringList parts{role.model.isEmpty() ? agentNames.value(role.agent, role.agent) : role.model};
            const QStringList where = !role.machines.isEmpty() ? role.machines : role.computer.isEmpty() ? QStringList() : QStringList{role.computer};
            if (!where.isEmpty() && !where.contains("any")) parts << where.join(", ");
            agentInfo->addWidget(logo); agentInfo->addWidget(ellipsis(parts.join(QStringLiteral(" · ")), c.muted, 12), 1);
            seats->addWidget(chip, line, 0); seats->addWidget(stepper, line, 1); seats->addWidget(only, line, 2); seats->addWidget(box(agentInfo), line++, 3);
        }
        seats->setColumnStretch(3, 1);
        rv->addLayout(seats);
        auto *foot = row(8);
        auto *summary = label(rosterSummary, c.text2, 12); summary->setObjectName("teamTemplateRosterSummary");
        foot->addWidget(summary, 1);
        auto *toggle = button(hideUnused ? tr("Show all catalog roles (%1)").arg(m_catalog.roles.size() - 1) : tr("Hide unused roles (%1)").arg(unused),
                              "teamTemplateUnusedToggle");
        toggle->setProperty("ghost", true);
        connect(toggle, &QPushButton::clicked, this, [this, hideUnused] {
            Settings::set(QStringLiteral("ui/catalogHideUnusedRoles"), !hideUnused); rebuildEditor();
        });
        foot->addWidget(toggle);
        auto *line2 = new QFrame; line2->setFrameShape(QFrame::HLine); line2->setStyleSheet(QStringLiteral("color:%1;").arg(c.border.name()));
        rv->addWidget(line2);
        rv->addLayout(foot);
    }
    add(foldLabel(tr("Roster"), QStringLiteral("roster"), "teamTemplateRosterToggle"), rosterBox);
    // Work in.
    auto *workBox = new QVBoxLayout; workBox->setSpacing(4);
    auto *workRow = row(0);
    const QString work = t.work.isEmpty() ? QStringLiteral("worktree") : t.work;
    workRow->addWidget(segmented({{"worktree", tr("Worktree per member")}, {"clone", tr("Own clone")}, {"same", tr("Same folder")}, {"cloud", tr("Cloud sandbox")}},
                                 work, "teamTemplateWork", [this, index](const QString &value) { m_catalog.templates[index].work = value; changed(); rebuildEditor(); }));
    workRow->addStretch();
    workBox->addLayout(workRow);
    workBox->addWidget(hint(work == "clone" ? tr("Each writer gets its own clone of the repository")
                            : work == "same" ? tr("Everyone works in the lead’s folder; safe only for read-only roles")
                            : work == "cloud" ? tr("Each member works in a cloud sandbox")
                                              : tr("Each writer gets its own git worktree and branch team/<name>/<role>"), c.muted));
    bool writers = false;
    for (const auto &seat : t.roster) if (const Role *role = m_catalog.role(seat.role); role && seat.count > 0 && (role->folder.isEmpty() || role->folder == "worktree")) writers = true;
    if (work == "same" && writers) workBox->addWidget(warningLine(tr("Writers share one folder: their edits can collide. Prefer worktrees."), c.warn, "teamTemplateWorkWarning"));
    addText(tr("Work in"), box(workBox));
    // Task skeleton: #, task, role, after #, phase.
    QStringList titles; int phases = 1;
    for (const auto &task : t.tasks) { titles << task.title; phases = qMax(phases, task.phase); }
    auto *skeleton = new QWidget; skeleton->setObjectName("teamTemplateSkeleton");
    auto *sv = new QVBoxLayout(skeleton); sv->setContentsMargins(0, 0, 0, 0); sv->setSpacing(5);
    QStringList issues;
    if (folded("skeleton")) {
        QSet<int> used; for (const auto &task : t.tasks) used.insert(task.phase);
        auto *summary = label(t.tasks.isEmpty() ? tr("No tasks: the lead plans")
                                                : tr("%1 in %2: %3").arg(counted(int(t.tasks.size()), tr("%1 task"), tr("%1 tasks")),
                                                                         counted(int(used.size()), tr("%1 phase"), tr("%1 phases")), titles.join(QStringLiteral(" → "))),
                              c.text2, 12);
        summary->setObjectName("teamTemplateSkeletonSummary"); summary->setWordWrap(true);
        sv->addWidget(summary);
    } else {
        auto *table = new QGridLayout; table->setHorizontalSpacing(8); table->setVerticalSpacing(5);
        int column = 0;
        for (const auto &text : {QStringLiteral("#"), tr("Task"), tr("Role"), tr("After #"), tr("Phase")}) table->addWidget(label(text, c.muted, 12), 0, column++);
        QList<std::pair<QString, QString>> taskRoles{{"lead", tr("Lead")}};
        for (const auto &role : m_catalog.roles) if (role.id != "lead") taskRoles.append({role.id, role.name});
        QList<std::pair<QString, QString>> phaseItems;
        for (int n = 1; n <= phases + 1; ++n) phaseItems.append({QString::number(n), tr("Phase %1").arg(n)});
        for (int i = 0; i < t.tasks.size(); ++i) {
            const auto &task = t.tasks[i];
            auto *title = new QLineEdit(task.title); title->setObjectName("teamTemplateTaskTitle");
            auto *role = choice(taskRoles, task.role, "teamTemplateTaskRole");
            QStringList depText; for (int d : task.deps) depText << QString::number(d + 1);
            auto *deps = new QLineEdit(depText.join(QStringLiteral(", "))); deps->setObjectName("teamTemplateTaskAfter"); deps->setPlaceholderText(tr("after #")); deps->setFixedWidth(90);
            auto *phase = choice(phaseItems, QString::number(task.phase), "teamTemplateTaskPhase"); phase->setFixedWidth(110);
            auto *del = iconButton(workspaceIcon("trash", c.muted), tr("Delete task"), "teamTemplateTaskDelete");
            connect(title, &QLineEdit::editingFinished, this, [this, index, i, title] {
                if (title->text().trimmed() == m_catalog.templates[index].tasks[i].title) return;
                m_catalog.templates[index].tasks[i].title = title->text().trimmed(); changed();
            });
            connect(role, &QComboBox::activated, this, [this, index, i, role] { m_catalog.templates[index].tasks[i].role = role->currentData().toString(); changed(); rebuildEditor(); });
            connect(deps, &QLineEdit::editingFinished, this, [this, index, i, deps] {
                QList<int> list; bool bad = false;
                for (const auto &part : deps->text().split(',', Qt::SkipEmptyParts)) {
                    const int n = part.trimmed().remove('#').toInt();
                    if (n >= 1 && n <= i) { if (!list.contains(n - 1)) list << n - 1; } else bad = true;
                }
                auto &task = m_catalog.templates[index].tasks[i];
                const bool same = task.deps == list && !bad;
                task.deps = list;
                setProperty(QStringLiteral("badAfter%1").arg(i).toUtf8().constData(), bad ? deps->text() : QString());
                if (!same) { changed(); rebuildEditor(); }
            });
            connect(phase, &QComboBox::activated, this, [this, index, i, phase] { m_catalog.templates[index].tasks[i].phase = phase->currentData().toString().toInt(); changed(); rebuildEditor(); });
            connect(del, &QToolButton::clicked, this, [this, index, i] {
                auto &tasks = m_catalog.templates[index].tasks;
                tasks.removeAt(i);
                for (auto &k : tasks) { QList<int> kept; for (int d : k.deps) if (d != i) kept << (d > i ? d - 1 : d); k.deps = kept; }
                changed(); rebuildEditor();
            });
            table->addWidget(label(QStringLiteral("#%1").arg(i + 1), c.muted, 12), i + 1, 0);
            table->addWidget(title, i + 1, 1); table->addWidget(role, i + 1, 2); table->addWidget(deps, i + 1, 3); table->addWidget(phase, i + 1, 4); table->addWidget(del, i + 1, 5);
            const QString badAfter = property(QStringLiteral("badAfter%1").arg(i).toUtf8().constData()).toString();
            if (!badAfter.isEmpty()) issues << tr("#%1: “after %2” must point to an earlier task").arg(i + 1).arg(badAfter);
        }
        table->setColumnStretch(1, 3); table->setColumnStretch(2, 2);
        sv->addLayout(table);
        auto *addTask = button(tr("+ Task"), "teamTemplateAddTask"); addTask->setProperty("ghost", true);
        connect(addTask, &QPushButton::clicked, this, [this, index] {
            auto &tasks = m_catalog.templates[index].tasks;
            SkeletonTask task{tr("New task"), QStringLiteral("lead"), tasks.isEmpty() ? QList<int>{} : QList<int>{int(tasks.size()) - 1}, {}, tasks.isEmpty() ? 1 : tasks.last().phase};
            tasks.append(task);
            changed(); rebuildEditor();
        });
        sv->addWidget(addTask, 0, Qt::AlignLeft);
    }
    add(foldLabel(tr("Task skeleton"), QStringLiteral("skeleton"), "teamTemplateSkeletonToggle"), skeleton);
    auto *planning = row(0);
    planning->addWidget(segmented({{"lead", tr("Lead plans")}, {"template", tr("Use template tasks")}}, t.planning.isEmpty() ? QStringLiteral("lead") : t.planning,
                                  "teamTemplatePlanning", [this, index](const QString &value) { m_catalog.templates[index].planning = value; changed(); }));
    planning->addStretch();
    addText(tr("Planning"), box(planning));
    auto *approval = row(0);
    approval->addWidget(segmented({{"review", tr("Review plan first")}, {"auto", tr("Run when the plan is valid")}}, t.approval.isEmpty() ? QStringLiteral("review") : t.approval,
                                  "teamTemplateApproval", [this, index](const QString &value) { m_catalog.templates[index].approval = value; changed(); }));
    approval->addStretch();
    addText(tr("Approval"), box(approval));
    // New phases: phase_approval for the server, phases (spec 12) alongside.
    auto *phaseApproval = row(0);
    const QString newPhases = !t.phaseApproval.isEmpty() ? t.phaseApproval : t.phases == "owner" ? QStringLiteral("review") : t.phases == "lead" ? QStringLiteral("auto") : QString();
    phaseApproval->addWidget(segmented({{"", tr("Same as plan")}, {"review", tr("Owner approves")}, {"auto", tr("Lead runs")}}, newPhases,
                                       "teamTemplatePhaseApproval", [this, index](const QString &value) {
                                           auto &templ = m_catalog.templates[index];
                                           templ.phaseApproval = value;
                                           templ.phases = value == "review" ? QStringLiteral("owner") : value == "auto" ? QStringLiteral("lead") : QStringLiteral("same");
                                           changed();
                                       }));
    phaseApproval->addStretch();
    addText(tr("New phases"), box(phaseApproval));
    auto *maxRow = row(0);
    maxRow->addWidget(new Stepper(t.maxMembers > 0 ? t.maxMembers : 6, 1, 32, "teamTemplateMax", [this, index](int n) { m_catalog.templates[index].maxMembers = n; changed(); rebuildEditor(); }));
    maxRow->addStretch();
    addText(tr("Max members"), box(maxRow));
    // Checks (spec 11.4).
    int seated = 1;
    for (const auto &seat : t.roster) seated += seat.count;
    if (seated > t.maxMembers) issues.prepend(tr("Lead + roster = %1 > max members %2").arg(seated).arg(t.maxMembers));
    for (int i = 0; i < t.tasks.size(); ++i) {
        const auto &task = t.tasks[i];
        if (task.role == "lead") continue;
        const Role *role = m_catalog.role(task.role);
        if (!role) issues << tr("#%1 uses %2, which was removed from the catalog").arg(i + 1).arg(task.role);
        else if (std::none_of(t.roster.cbegin(), t.roster.cend(), [&](const Seat &s) { return s.role == task.role && s.count > 0; }))
            issues << tr("#%1 uses %2, which is not in the roster").arg(i + 1).arg(role->name);
    }
    for (const auto &seat : t.roster) if (!m_catalog.role(seat.role)) issues << tr("The roster names %1, which was removed from the catalog").arg(seat.role);
    if (!issues.isEmpty()) {
        auto *list = new QVBoxLayout; list->setSpacing(3);
        for (const auto &issue : issues) list->addWidget(warningLine(issue, c.warn, "teamTemplateIssue"));
        grid->addWidget(box(list), at++, 1);
    }
    if (m_readOnly) form->setEnabled(false);
    outer->addWidget(form);
    connect(duplicate, &QPushButton::clicked, this, [this, index] {
        Template copy = m_catalog.templates[index]; copy.name += tr(" copy"); copy.id = slug(copy.name);
        for (int n = 2; m_catalog.templ(copy.id); ++n) copy.id = slug(copy.name) + QStringLiteral("-%1").arg(n);
        m_catalog.templates.insert(index + 1, copy); changed(); rebuildLists(); m_templates->setCurrentRow(index + 1);
    });
    connect(remove, &QPushButton::clicked, this, [this, index] {
        if (confirm && !confirm(QStringLiteral("teamTemplateDeleteConfirm"), tr("Delete template"),
                                tr("Delete the template %1 and its routing rules?").arg(m_catalog.templates[index].name), tr("Delete"))) return;
        const QString id = m_catalog.templates[index].id;
        m_catalog.templates.removeAt(index);
        m_catalog.routing.erase(std::remove_if(m_catalog.routing.begin(), m_catalog.routing.end(), [&](const Rule &rule) { return !rule.use.startsWith('+') && rule.templateId == id; }), m_catalog.routing.end());
        changed(); rebuildLists(); rebuildEditor();
    });
    return w;
}

// U28: the lead's planning rules as one text, saved and reverted with the
// catalog. A designer proposal on them shows as CHANGED with Keep / Reject /
// Undo and the lines it removes and adds.
QWidget *TeamCatalogEditor::planningRulesEditor()
{
    const Colors c = colorsFor(m_dark);
    const auto &base = m_teams->catalog();
    auto *w = new QFrame; w->setObjectName("teamCatalogCard");
    auto *outer = new QVBoxLayout(w); outer->setContentsMargins(16, 14, 16, 16); outer->setSpacing(12);
    auto *head = row(8);
    auto *title = label(tr("Planning rules"), c.fg, 14, true, "teamPlanningRulesTitle");
    head->addWidget(title);
    head->addWidget(ellipsis(tr("The lead follows them in every plan; the designer and the plan health refer to them."), c.muted, 13), 1);
    auto *reset = button(tr("Reset to built-in"), "teamPlanningRulesReset"); reset->setProperty("ghost", true);
    reset->setIcon(workspaceIcon("reset", c.text2));
    reset->setVisible(!m_readOnly);
    connect(reset, &QPushButton::clicked, this, [this] { resetPlanningRules(); });
    head->addWidget(reset);
    outer->addLayout(head);
    updatePlanningRulesReset();
    const Proposal *proposal = nullptr;
    if (draftMode()) for (const auto &p : base.proposals) if (p.kind == "planning_rules") proposal = &p;
    if (proposal) {
        outer->addWidget(proposalPlate(*proposal));
        // The proposal's diff, line by line (a longest common subsequence).
        const QStringList before = base.planningRules.split(QLatin1Char('\n')), after = proposal->text.split(QLatin1Char('\n'));
        const int n = before.size(), m = after.size();
        QVector<QVector<int>> common(n + 1, QVector<int>(m + 1, 0));
        for (int i = n - 1; i >= 0; --i)
            for (int j = m - 1; j >= 0; --j)
                common[i][j] = before[i] == after[j] ? common[i + 1][j + 1] + 1 : qMax(common[i + 1][j], common[i][j + 1]);
        auto *diff = new QFrame; diff->setObjectName("teamPlanningRulesDiff");
        auto *dv = new QVBoxLayout(diff); dv->setContentsMargins(0, 0, 0, 0); dv->setSpacing(2);
        const bool rejected = proposal->state == "rejected";
        const auto line = [&](const QString &text, const char *name, const QColor &ink, const QColor &fill, bool struck) {
            auto *l = label(text.isEmpty() ? QStringLiteral(" ") : text, ink, 12, false, name); l->setWordWrap(true);
            if (struck) { QFont f = l->font(); f.setStrikeOut(true); l->setFont(f); }
            if (fill.isValid()) l->setStyleSheet(QStringLiteral("QLabel{background:%1;padding:1px 6px;border-radius:3px;}").arg(fill.name(QColor::HexArgb)));
            else l->setStyleSheet(QStringLiteral("QLabel{padding:1px 6px;}"));
            dv->addWidget(l);
        };
        int i = 0, j = 0;
        while (i < n || j < m) {
            if (i < n && j < m && before[i] == after[j]) { line(before[i], "teamPlanningRulesSame", c.text2, QColor(), false); ++i; ++j; }
            // A replaced line reads old, then new.
            else if (i < n && (j == m || common[i + 1][j] >= common[i][j + 1])) {
                line(before[i], "teamPlanningRulesRemoved", rejected ? c.dim : c.text2, rejected ? QColor() : alpha(c.danger, .14), !rejected); ++i;
            } else { line(after[j], "teamPlanningRulesAdded", rejected ? c.dim : c.fg, rejected ? QColor() : alpha(c.accent, .14), rejected); ++j; }
        }
        outer->addWidget(diff);
        outer->addStretch();
        return w;
    }
    if (!m_catalog.planningRulesKnown) {
        outer->addWidget(hint(tr("This hgs does not report planning rules yet; update it to edit them here."), c.muted));
        return w;
    }
    auto *text = area(m_catalog.planningRules, "teamPlanningRules", 12, 30);
    text->setFont(NewSessionDense::mono(12));
    text->setReadOnly(m_readOnly);
    text->setPlaceholderText(tr("One rule per line, such as (1) At most 20-25 tasks per phase…"));
    connect(text, &QPlainTextEdit::textChanged, this, [this, text] {
        if (text->toPlainText() == m_catalog.planningRules) return;
        m_catalog.planningRules = text->toPlainText(); changed();
    });
    outer->addWidget(text);
    outer->addStretch();
    return w;
}

// Reset saves over the catalog as hgs has it: never while edits are unsaved,
// so the button follows every edit, not only a rebuild.
bool TeamCatalogEditor::canResetPlanningRules() const
{
    return !m_dirty && m_teams->catalog().planningRulesKnown;
}

void TeamCatalogEditor::updatePlanningRulesReset()
{
    auto *reset = findChild<QPushButton *>(QStringLiteral("teamPlanningRulesReset"));
    if (!reset) return;
    reset->setEnabled(canResetPlanningRules());
    reset->setToolTip(m_dirty ? tr("Save or revert your edits first") : tr("Replace the rules with Zerus's built-in ones and save the catalog"));
}

// U28: the built-in rules come back by saving the catalog without them; hgs
// fills in its own. Only from a saved catalog, after the owner confirms.
void TeamCatalogEditor::resetPlanningRules()
{
    const auto &base = m_teams->catalog();
    if (!canResetPlanningRules()) return;
    if (confirm && !confirm(QStringLiteral("teamPlanningRulesResetConfirm"), tr("Reset planning rules"),
                            tr("Replace the planning rules with the built-in ones and save the catalog as v%1?").arg(base.version + 1), tr("Reset"))) return;
    QJsonObject catalog = base.json();
    catalog.remove(QStringLiteral("planning_rules"));
    m_teams->call(QStringLiteral("catalog set"), {{"catalog", catalog}, {"version", base.version}}, [this](bool ok, const QJsonObject &result, const QString &error) {
        if (!ok) { m_meta->setText(QStringLiteral("⚠ ") + error.toHtmlEscaped()); return; }
        m_applied = tr("Planning rules reset to built-in · saved as v%1").arg(result.value("version").toInteger());
        updateHeader();
        m_teams->refreshCatalog();
    });
}

QWidget *TeamCatalogEditor::routingEditor()
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QWidget; w->setObjectName("teamRoutingEditor");
    auto *v = new QVBoxLayout(w); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(10);
    v->addWidget(hint(tr("Rules run top to bottom. The first rule whose words appear in the goal or issue picks the template; “+ role” rules add a member and keep looking."), c.muted));
    auto *table = new QGridLayout; table->setHorizontalSpacing(10); table->setVerticalSpacing(8);
    int column = 1;
    for (const auto &text : {QStringLiteral("#"), tr("When the goal / issue has"), tr("Use"), tr("Approval")}) table->addWidget(label(text, c.muted, 12), 0, column++);
    QList<std::pair<QString, QString>> uses;
    for (const auto &t : m_catalog.templates) uses.append({t.id, t.name});
    for (const auto &role : m_catalog.roles) if (role.id != "lead") uses.append({"+" + role.id, tr("+ %1 (add member)").arg(role.name)});
    for (int i = 0; i < m_catalog.routing.size(); ++i) {
        const Rule &rule = m_catalog.routing[i];
        auto *moves = new QVBoxLayout; moves->setSpacing(0);
        auto *up = iconButton(turned("chevron-down", c.text2, 180), tr("Move up"), "teamRuleUp"); up->setEnabled(!m_readOnly && i > 0);
        auto *down = iconButton(turned("chevron-down", c.text2, 0), tr("Move down"), "teamRuleDown"); down->setEnabled(!m_readOnly && i + 1 < m_catalog.routing.size());
        for (auto *b : {up, down}) { b->setFixedSize(20, 15); b->setIconSize(QSize(11, 11)); }
        moves->addWidget(up); moves->addWidget(down);
        auto *when = new QLineEdit(rule.when); when->setObjectName("teamRuleWhen");
        const QString use = rule.use.startsWith('+') ? rule.use : rule.templateId;
        auto *templ = choice(uses, use, "teamRuleTemplate");
        auto *approval = choice({{"review", tr("Review plan first")}, {"auto", tr("Run when the plan is valid")}, {"", tr("Template default")}}, rule.approval, "teamRuleApproval");
        auto *del = iconButton(workspaceIcon("trash", c.muted), tr("Delete rule"), "teamRuleDelete");
        connect(when, &QLineEdit::editingFinished, this, [this, i, when] {
            if (when->text().trimmed() == m_catalog.routing[i].when) return;
            m_catalog.routing[i].when = when->text().trimmed(); changed();
        });
        connect(templ, &QComboBox::activated, this, [this, i, templ] {
            auto &r = m_catalog.routing[i]; const QString value = templ->currentData().toString();
            if (value.startsWith('+')) { r.use = value; }
            else { r.templateId = value; r.use = r.use.isEmpty() && !r.raw.contains("use") ? QString() : value; }
            changed(); rebuildEditor();
        });
        connect(approval, &QComboBox::activated, this, [this, i, approval] { m_catalog.routing[i].approval = approval->currentData().toString(); changed(); });
        connect(up, &QToolButton::clicked, this, [this, i] { m_catalog.routing.swapItemsAt(i, i - 1); changed(); rebuildEditor(); });
        connect(down, &QToolButton::clicked, this, [this, i] { m_catalog.routing.swapItemsAt(i, i + 1); changed(); rebuildEditor(); });
        connect(del, &QToolButton::clicked, this, [this, i] { m_catalog.routing.removeAt(i); changed(); rebuildEditor(); });
        for (auto *field : std::initializer_list<QWidget *>{when, templ, approval, del}) field->setEnabled(!m_readOnly);
        table->addLayout(moves, i + 1, 0);
        table->addWidget(label(QString::number(i + 1), c.muted, 13), i + 1, 1);
        table->addWidget(when, i + 1, 2); table->addWidget(templ, i + 1, 3); table->addWidget(approval, i + 1, 4);
        if (const auto *p = proposalOf("rule", rule.id)) table->addWidget(mark(*p, c), i + 1, 5);
        table->addWidget(del, i + 1, 6);
    }
    table->setColumnStretch(2, 4); table->setColumnStretch(3, 3); table->setColumnStretch(4, 2);
    v->addLayout(table);
    // The draft's rules decide on their own plates.
    for (const auto &rule : m_catalog.routing) if (const auto *p = proposalOf("rule", rule.id)) v->addWidget(proposalPlate(*p));
    auto *addRule = button(tr("+ Rule"), "teamRoutingAdd"); addRule->setEnabled(!m_readOnly && !m_catalog.templates.isEmpty());
    connect(addRule, &QPushButton::clicked, this, [this] {
        Rule r; r.id = QStringLiteral("rule-%1").arg(m_catalog.routing.size() + 1);
        while (std::any_of(m_catalog.routing.cbegin(), m_catalog.routing.cend(), [&](const Rule &x) { return x.id == r.id; })) r.id += 'x';
        r.templateId = m_catalog.templates.first().id; r.approval = "review";
        m_catalog.routing.append(r); changed(); rebuildEditor();
    });
    v->addWidget(addRule, 0, Qt::AlignLeft);
    // Try it: which team Zerus would build for a goal.
    auto *tryBox = new QFrame; tryBox->setObjectName("teamRoutingTryBox");
    auto *tv = new QVBoxLayout(tryBox); tv->setContentsMargins(14, 12, 14, 14); tv->setSpacing(8);
    auto *title = new QLabel(QStringLiteral("<b>%1</b> <span style='color:%2;'>— %3</span>").arg(tr("Try it").toHtmlEscaped(), c.muted.name(), tr("which team would Zerus build?").toHtmlEscaped()));
    title->setTextFormat(Qt::RichText); title->setStyleSheet(QStringLiteral("color:%1;font-size:13px;").arg(c.fg.name()));
    tv->addWidget(title);
    auto *goal = new QLineEdit; goal->setObjectName("teamRoutingTry"); goal->setPlaceholderText(tr("Type a goal or an issue, e.g. Fix rounding in tax total (label: bug)"));
    goal->setText(property("tryGoal").toString());
    auto *result = new QLabel; result->setObjectName("teamRoutingResult"); result->setTextFormat(Qt::RichText); result->setWordWrap(true);
    auto *rosterBox = new QWidget; rosterBox->setObjectName("teamRoutingRoster"); auto *roster = new FlowLayout(6); rosterBox->setLayout(roster);
    const auto show = [this, result, roster, c](const QString &text) {
        setProperty("tryGoal", text);
        while (auto *item = roster->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
        if (text.trimmed().isEmpty()) { result->clear(); return; }
        const Route found = route(m_catalog, text);
        const Template *t = m_catalog.templ(found.templateId);
        if (!t) { result->setText(QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.muted.name(), tr("No templates"))); return; }
        QStringList added; for (const auto &id : found.added) if (const Role *r = m_catalog.role(id)) added << r->name;
        QString line = QStringLiteral("<span style='color:%1;'>→</span> <b style='color:%2;'>%3</b> <span style='color:%4;'>%5")
                           .arg(c.accent.name(), c.fg.name(), t->name.toHtmlEscaped(), c.muted.name(),
                                found.rule >= 0 ? tr("rule %1").arg(found.rule + 1) : tr("no rule matched"));
        if (!added.isEmpty()) line += QStringLiteral(" (+ %1)").arg(added.join(", ").toHtmlEscaped());
        line += QStringLiteral(" · %1 · %2</span>").arg(approvalText(found.approval), workText(t->work).left(1).toUpper() + workText(t->work).mid(1));
        result->setText(line);
        if (const Role *lead = m_catalog.role("lead")) roster->addWidget(chipOf(*lead, m_dark));
        for (const auto &seat : t->roster) if (const Role *r = m_catalog.role(seat.role); r && seat.count > 0)
            roster->addWidget(chipOf(*r, m_dark, (seat.count > 1 ? QStringLiteral(" ×%1").arg(seat.count) : QString()) + (seat.optional ? QStringLiteral("?") : QString()), seat.optional));
        for (const auto &id : found.added) if (const Role *r = m_catalog.role(id)) roster->addWidget(chipOf(*r, m_dark, QStringLiteral(" +")));
    };
    connect(goal, &QLineEdit::textChanged, this, show);
    show(goal->text());
    tv->addWidget(goal); tv->addWidget(result); tv->addWidget(rosterBox);
    v->addWidget(tryBox);
    return w;
}

void TeamCatalogEditor::showProposal(const QString &proposal)
{
    for (const auto &p : m_teams->catalog().proposals) if (p.id == proposal) {
        setTab(p.kind == "role" ? Tab::Roles : p.kind == "template" ? Tab::Templates : p.kind == "planning_rules" ? Tab::Rules
               : p.kind == "pipeline" ? Tab::Pipelines : Tab::Routing);
        auto *list = p.kind == "role" ? m_roles : p.kind == "pipeline" ? m_pipelines : m_templates;
        if (p.kind != "rule") for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(Qt::UserRole).toString() == p.target) list->setCurrentRow(i);
        return;
    }
}

void TeamCatalogEditor::applyKept()
{
    // U15: from the draft's own state, not this editor's mode: the designer
    // page applies while a hand edit hides the draft here.
    const auto &base = m_teams->catalog();
    int kept = 0, undecided = 0;
    for (const auto &p : base.proposals) { kept += p.state == "kept"; undecided += p.state == "proposed"; }
    if (!kept || base.draftBase != base.version) return;
    // N37 (#548): Apply ends the whole draft (catalog.rs, "apply": save_draft
    // of an empty draft); undecided changes would be based on the old version.
    const QString dropped = undecided ? counted(undecided, tr("The %1 undecided change is dropped with the draft."),
                                                tr("The %1 undecided changes are dropped with the draft.")) + QLatin1Char(' ') : QString();
    if (confirm && !confirm(QStringLiteral("teamCatalogApplyConfirm"), tr("Apply changes"), counted(kept, tr("Write %1 kept change as the next catalog version?"), tr("Write %1 kept changes as the next catalog version?")) + QLatin1Char(' ') + dropped + tr("Running teams keep their roster."), tr("Apply"))) return;
    m_teams->applyDraft([this](bool ok, const QJsonObject &result, const QString &) {
        if (!ok) return;   // the controller keeps the refusal for the banner
        m_applied = tr("Applied as v%1").arg(result.value("version").toInteger());
        updateHeader();
    });
}

void TeamCatalogEditor::setSources(HgsClient *client, const QList<Computer> &computers)
{
    m_computers = computers;
    if (client && client != m_client) {
        m_client = client;
        m_modelCatalogs.clear(); m_modelPending.clear(); m_modelRequests.clear();
        connect(m_client, &HgsClient::accountsReady, this, [this](quint64 request, const QString &host, const QJsonObject &result) {
            if (m_modelRequests.contains(request)) {
                const QString key = m_modelRequests.take(request); m_modelPending.remove(key);
                m_modelCatalogs.insert(key, result.value("models").toArray());
                if (m_modelChoice && m_modelRole >= 0 && m_modelRole < m_catalog.roles.size()
                    && modelKey(m_catalog.roles[m_modelRole]) == key)
                    fillModels(m_modelChoice, m_catalog.roles[m_modelRole]);
                return;
            }
            if (host != QString() && std::none_of(m_computers.cbegin(), m_computers.cend(), [&](const Computer &c) { return c.alias == host; })) return;
            QList<Account> list;
            for (const auto &value : result.value("profiles").toArray()) {
                const auto p = value.toObject();
                list.append({p.value("id").toString(), p.value("provider").toString(), p.value("label").toString(),
                             p.value("account_status").toObject().value("status").toString() != "signed_out"});
            }
            m_accounts.insert(host, list);
            if (m_accountChoice && m_accountRole >= 0 && m_accountRole < m_catalog.roles.size()) fillAccounts(m_accountChoice, m_catalog.roles[m_accountRole]);
        });
        connect(m_client, &HgsClient::accountsFailed, this, [this](quint64 request, const QString &, const QString &) {
            const QString key = m_modelRequests.take(request); m_modelPending.remove(key);
        });
    }
    if (!m_client) return;
    m_client->requestAccounts({});
    for (const auto &c : m_computers) if (!c.alias.isEmpty() && c.online) m_client->requestAccounts(c.alias);
}

void TeamCatalogEditor::fillAccounts(QComboBox *box, const Teams::Role &role) const
{
    const QSignalBlocker blocker(box);
    box->clear(); box->addItem(tr("Default account"), QString());
    const auto agent = role.agent.isEmpty() ? QStringLiteral("claude") : role.agent;
    for (const auto &a : m_accounts.value(role.computer)) if (a.provider == agent)
        box->addItem((a.label.isEmpty() ? a.id : a.label) + (a.signedIn ? QString() : tr(" (signed out)")), a.id);
    // An account this computer did not report (yet) stays as written.
    if (!role.account.isEmpty() && box->findData(role.account) < 0) box->addItem(role.account, role.account);
    box->setCurrentIndex(qMax(0, box->findData(role.account)));
}

QString TeamCatalogEditor::modelKey(const Teams::Role &role) const
{
    const QString agent = role.agent.isEmpty() ? QStringLiteral("claude") : role.agent;
    return role.computer + QLatin1Char('\n') + (role.account.isEmpty() ? QStringLiteral("native-") + agent : role.account);
}

void TeamCatalogEditor::fillModels(QComboBox *box, const Teams::Role &role) const
{
    const QSignalBlocker blocker(box);
    box->clear();
    const auto models = m_modelCatalogs.value(modelKey(role));
    if (models.isEmpty()) box->addItems(Teams::knownModels(role.agent));
    for (const auto &value : models) {
        const QString id = value.toObject().value("id").toString();
        if (!id.isEmpty() && box->findText(id) < 0) box->addItem(id, id);
    }
    if (!role.model.isEmpty() && box->findText(role.model) < 0) box->addItem(role.model, role.model);
    box->setCurrentText(role.model);
}

void TeamCatalogEditor::requestModels(const Teams::Role &role)
{
    const QString agent = role.agent.isEmpty() ? QStringLiteral("claude") : role.agent;
    if (!m_client || !QStringList{"claude", "codex", "kimi", "dsh"}.contains(agent)) return;
    const QString key = modelKey(role);
    if (m_modelCatalogs.contains(key) || m_modelPending.contains(key)) return;
    const QString account = key.section(QLatin1Char('\n'), 1);
    m_modelPending.insert(key);
    const quint64 request = m_client->requestAccounts(role.computer, {"model-settings", account, "--json"});
    m_modelRequests.insert(request, key);
}

// U36: the templates and roles that name the preset as their default.
QStringList TeamCatalogEditor::pipelineUsers(const QString &id) const
{
    QStringList users;
    for (const auto &t : m_catalog.templates) if (t.pipeline == id) users << t.name;
    for (const auto &r : m_catalog.roles) if (r.pipeline == id) users << r.name;
    return users;
}

// U36 (mock-up #530): a preset's name, its stages after the implementer's
// report (kind, who takes it, where it works), the final acceptance and the
// rounds, then the defaults per template and role and Try it. -1 is None.
QWidget *TeamCatalogEditor::pipelineEditor(int index)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QFrame; w->setObjectName("teamCatalogCard");
    auto *outer = new QVBoxLayout(w); outer->setContentsMargins(16, 14, 16, 16); outer->setSpacing(12);
    if (index < 0) {
        outer->addWidget(label(tr("None"), c.fg, 15, true, "teamPipelineTitle"));
        outer->addWidget(hint(tr("A task without a pipeline goes from its report to the lead (or the member's sub-lead), as today. A template or role "
                                 "without a default gives its tasks none; the plan and edit-draft can choose None for a single task."), c.text2));
        outer->addStretch();
        return w;
    }
    Pipeline &p = m_catalog.pipelines[index];
    const Pipeline *orig = m_readOnly ? nullptr : m_teams->catalog().pipeline(p.id);
    const QString id = p.id;
    const auto at = [this, id]() -> Pipeline * {
        for (auto &x : m_catalog.pipelines) if (x.id == id) return &x;
        return nullptr;
    };
    // Header: name, whose default it is, Duplicate, Delete.
    auto *head = row(8);
    head->addWidget(label(p.name, c.fg, 15, true, "teamPipelineTitle"));
    const QStringList users = pipelineUsers(p.id);
    auto *usedBy = ellipsis(users.isEmpty() ? tr("· no template or role names it") : tr("· default of %1").arg(users.join(", ")), c.muted, 13);
    usedBy->setObjectName("teamPipelineUsedBy");
    head->addWidget(usedBy, 1);
    auto *duplicate = button(tr("Duplicate"), "teamPipelineDuplicate"); duplicate->setProperty("ghost", true); duplicate->setIcon(workspaceIcon("copy", c.text2));
    auto *remove = button(tr("Delete"), "teamPipelineDelete"); remove->setProperty("danger", true); remove->setIcon(workspaceIcon("trash", c.danger));
    duplicate->setEnabled(!m_readOnly && m_catalog.pipelines.size() < 20); remove->setEnabled(!m_readOnly);
    head->addWidget(duplicate); head->addWidget(remove);
    outer->addLayout(head);
    connect(duplicate, &QPushButton::clicked, this, [this, index] {
        Pipeline copy = m_catalog.pipelines[index]; copy.name += tr(" copy"); copy.id = slug(copy.name); copy.raw.remove("id");
        for (int n = 2; m_catalog.pipeline(copy.id); ++n) copy.id = slug(copy.name) + QStringLiteral("-%1").arg(n);
        m_catalog.pipelines.insert(index + 1, copy); changed(); rebuildLists(); m_pipelines->setCurrentRow(index + 1);
    });
    connect(remove, &QPushButton::clicked, this, [this, index] {
        const auto &doomed = m_catalog.pipelines[index];
        const QStringList losing = pipelineUsers(doomed.id);
        const QString text = losing.isEmpty() ? tr("Delete the pipeline %1? Running tasks keep their snapshot.").arg(doomed.name)
                                              : tr("Delete the pipeline %1? These lose it as their default: %2. Running tasks keep their snapshot.").arg(doomed.name, losing.join(", "));
        if (confirm && !confirm(QStringLiteral("teamPipelineDeleteConfirm"), tr("Delete pipeline"), text, tr("Delete"))) return;
        const QString gone = doomed.id;
        for (auto &t : m_catalog.templates) if (t.pipeline == gone) t.pipeline.clear();
        for (auto &r : m_catalog.roles) if (r.pipeline == gone) r.pipeline.clear();
        m_catalog.pipelines.removeAt(index); changed(); rebuildLists(); rebuildEditor();
    });
    if (const auto *proposal = proposalOf("pipeline", p.id)) {
        outer->addWidget(proposalPlate(*proposal));
        if (auto *diff = pipelineDiff(p, *proposal)) outer->addWidget(diff);
    }
    auto *form = new QWidget; form->setObjectName("teamPipelineEditor");
    auto *grid = new QGridLayout(form); grid->setContentsMargins(0, 0, 0, 0); grid->setHorizontalSpacing(14); grid->setVerticalSpacing(10);
    grid->setColumnMinimumWidth(0, labelWidth); grid->setColumnStretch(1, 1);
    int line = 0;
    const auto add = [&](const QString &text, bool edited, QWidget *field) {
        grid->addWidget(fieldLabel(text, edited, c), line, 0, Qt::AlignTop | Qt::AlignRight); grid->addWidget(field, line++, 1);
    };
    auto *name = new QLineEdit(p.name); name->setObjectName("teamPipelineName"); name->setReadOnly(m_readOnly); name->setMaxLength(80);
    connect(name, &QLineEdit::editingFinished, this, [this, at, name] {
        auto *x = at(); const auto text = name->text().trimmed();
        if (!x || text.isEmpty() || text == x->name) return;
        x->name = text; changed(); rebuildEditor();
    });
    add(tr("Name"), orig && orig->name != p.name, name);
    // Stages: the implementer first, then each stage.
    auto *stagesBox = new QWidget; stagesBox->setObjectName("teamPipelineStages");
    auto *stages = new QGridLayout(stagesBox); stages->setContentsMargins(0, 0, 0, 0); stages->setHorizontalSpacing(10); stages->setVerticalSpacing(8);
    int column = 1;
    for (const auto &text : {QStringLiteral("#"), tr("Kind"), tr("Who takes it"), tr("Works in")}) stages->addWidget(label(text, c.muted, 12), 0, column++);
    stages->addWidget(label(QStringLiteral("0"), c.muted, 13), 1, 1);
    stages->addWidget(new StageChip("implement", stageName("implement"), stageColor("implement", c)), 1, 2, Qt::AlignLeft);
    stages->addWidget(label(tr("the task's assignee"), c.muted, 12), 1, 3);
    stages->addWidget(label(tr("its own folder"), c.muted, 12), 1, 4);
    int open = m_openStage.value(p.id, -1);
    if (!m_openStage.contains(p.id))
        for (int i = 0; i < p.stages.size() && open < 0; ++i) if (p.stages[i].kind != "review") open = i;
    QStringList problems;
    int gridRow = 2;
    QList<std::pair<QString, QString>> roleItems;
    for (const auto &r : m_catalog.roles) if (r.id != "lead") roleItems.append({QStringLiteral("role:") + r.id, tr("first free %1").arg(r.name)});
    for (int i = 0; i < p.stages.size(); ++i) {
        const PipelineStage &s = p.stages[i];
        const bool land = s.kind == "land";
        const bool nextIsLand = i + 1 < p.stages.size() && p.stages[i + 1].kind == "land";
        auto *moves = new QVBoxLayout; moves->setSpacing(0);
        auto *up = iconButton(turned("chevron-down", c.text2, 180), tr("Move up"), "teamPipelineStageUp"); up->setEnabled(!m_readOnly && i > 0 && !land);
        auto *down = iconButton(turned("chevron-down", c.text2, 0), tr("Move down"), "teamPipelineStageDown");
        down->setEnabled(!m_readOnly && i + 1 < p.stages.size() && !nextIsLand);
        for (auto *b : {up, down}) { b->setFixedSize(20, 15); b->setIconSize(QSize(11, 11)); }
        moves->addWidget(up); moves->addWidget(down);
        connect(up, &QToolButton::clicked, this, [this, at, i] { if (auto *x = at()) { x->stages.swapItemsAt(i, i - 1); m_openStage[x->id] = i - 1; changed(); rebuildEditor(); } });
        connect(down, &QToolButton::clicked, this, [this, at, i] { if (auto *x = at()) { x->stages.swapItemsAt(i, i + 1); m_openStage[x->id] = i + 1; changed(); rebuildEditor(); } });
        // The number opens the stage's details.
        auto *number = button(QString::number(i + 1), "teamPipelineStageOpen"); number->setProperty("ghost", true);
        number->setCheckable(true); number->setChecked(open == i); number->setFixedWidth(30);
        number->setToolTip(open == i ? tr("Hide the stage's details") : tr("Show the stage's details"));
        number->setStyleSheet(QStringLiteral("QPushButton{color:%1;font-size:13px;padding:2px 0;}").arg((open == i ? c.accent : c.muted).name()));
        connect(number, &QPushButton::clicked, this, [this, id, i, open] { m_openStage[id] = open == i ? -1 : i; rebuildEditor(); });
        auto *kind = segmented({{"review", tr("Review")}, {"test", tr("Test")}, {"command", tr("Command")}, {"land", tr("Land")}}, s.kind, "teamPipelineStageKind",
                               [this, at, i](const QString &value) {
            auto *x = at(); if (!x || x->stages[i].kind == value) return;
            auto &stage = x->stages[i];
            stage.kind = value;
            if (stage.runByZerus()) { stage.role.clear(); stage.member.clear(); stage.hasCommand = true; }
            else if (stage.role.isEmpty() && stage.member.isEmpty()) stage.role = defaultStageRole(m_catalog, value);
            if (value != "land") stage.exits.clear();
            m_openStage[x->id] = i;
            changed(); rebuildEditor();
        });
        // A land stage is the last one, and only one.
        bool otherLand = false;
        for (int j = 0; j < p.stages.size(); ++j) otherLand = otherLand || (j != i && p.stages[j].kind == "land");
        for (auto *b : kind->findChildren<QPushButton *>()) {
            if (b->property("segment") == "land" && !land && (i + 1 != p.stages.size() || otherLand)) {
                b->setEnabled(false); b->setToolTip(tr("A land stage is the pipeline's last stage"));
            }
            if (b->property("segment") == "command" || b->property("segment") == "land") b->setToolTip(b->isEnabled() ? tr("A stage Zerus runs itself, with no model") : b->toolTip());
        }
        kind->setEnabled(!m_readOnly);
        QWidget *who = nullptr;
        if (s.runByZerus()) {
            who = label(tr("Zerus runs it, with no model"), c.muted, 12, false, "teamPipelineStageZerus");
            if (s.command.trimmed().isEmpty()) problems << tr("Stage %1: a %2 stage needs its command").arg(i + 1).arg(stageName(s.kind).toLower());
        } else {
            auto items = roleItems;
            if (!s.member.isEmpty()) items.append({QStringLiteral("member"), tr("pinned %1").arg(s.member)});
            else items.append({QStringLiteral("member"), tr("Pinned member…")});
            const QString current = s.member.isEmpty() && !s.role.isEmpty() ? QStringLiteral("role:") + s.role : QStringLiteral("member");
            auto *roleBox = choice(items, current, "teamPipelineStageRole");
            for (int k = 0; k < roleBox->count(); ++k) {
                const QString data = roleBox->itemData(k).toString();
                if (const Role *r = data.startsWith("role:") ? m_catalog.role(data.mid(5)) : nullptr) roleBox->setItemIcon(k, roleIcon(iconOf(*r), colorOf(*r, m_dark)));
            }
            roleBox->setEnabled(!m_readOnly);
            roleBox->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon); roleBox->setMinimumContentsLength(8);
            connect(roleBox, &QComboBox::activated, this, [this, at, i, roleBox] {
                auto *x = at(); if (!x) return;
                auto &stage = x->stages[i];
                const QString value = roleBox->currentData().toString();
                if (value == "member") { stage.role.clear(); m_openStage[x->id] = i; }
                else { stage.role = value.mid(5); stage.member.clear(); }
                changed(); rebuildEditor();
            });
            who = roleBox;
            if (s.role.isEmpty() && s.member.isEmpty()) problems << tr("Stage %1: name the member who takes it, or choose a role").arg(i + 1);
        }
        QWidget *where = nullptr;
        if (s.kind == "test") {
            auto *folder = choice({{"hold", folderName("hold")}, {"checkout", folderName("checkout")}}, p.folder.isEmpty() ? QStringLiteral("hold") : p.folder, "teamPipelineStageFolder");
            folder->setEnabled(!m_readOnly);
            folder->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon); folder->setMinimumContentsLength(10);
            connect(folder, &QComboBox::activated, this, [this, at, folder] { if (auto *x = at()) { x->folder = folder->currentData().toString(); changed(); rebuildEditor(); } });
            where = folder;
        } else {
            where = label(s.kind == "review" ? tr("Task's folder · reads") : s.kind == "land" ? tr("Task's folder · lands") : tr("Task's folder · runs"), c.text2, 12);
        }
        auto *del = iconButton(workspaceIcon("trash", c.muted), tr("Delete stage"), "teamPipelineStageDelete"); del->setEnabled(!m_readOnly);
        connect(del, &QToolButton::clicked, this, [this, at, i] { if (auto *x = at()) { x->stages.removeAt(i); m_openStage[x->id] = -1; changed(); rebuildEditor(); } });
        stages->addLayout(moves, gridRow, 0); stages->addWidget(number, gridRow, 1); stages->addWidget(kind, gridRow, 2, Qt::AlignLeft);
        stages->addWidget(who, gridRow, 3); stages->addWidget(where, gridRow, 4); stages->addWidget(del, gridRow, 5);
        ++gridRow;
        if (open == i) stages->addWidget(pipelineStageDetails(index, i), gridRow++, 2, 1, 4);
    }
    stages->setColumnStretch(3, 3); stages->setColumnStretch(4, 2);
    auto *stageColumn = new QVBoxLayout; stageColumn->setSpacing(8);
    stageColumn->addWidget(stagesBox);
    auto *addRow = row(10);
    auto *addStage = button(tr("+ Stage"), "teamPipelineAddStage"); addStage->setProperty("ghost", true);
    addStage->setEnabled(!m_readOnly && p.stages.size() < 8);
    addStage->setToolTip(p.stages.size() < 8 ? QString() : tr("A pipeline has at most 8 stages"));
    connect(addStage, &QPushButton::clicked, this, [this, at] {
        auto *x = at(); if (!x) return;
        PipelineStage stage; stage.kind = "review"; stage.role = defaultStageRole(m_catalog, "review");
        // Before a land stage, which stays last.
        int place = x->stages.size();
        if (place > 0 && x->stages.last().kind == "land") --place;
        x->stages.insert(place, stage); m_openStage[x->id] = place;
        changed(); rebuildEditor();
    });
    addRow->addWidget(addStage);
    addRow->addWidget(hint(tr("Review reads the attempt's change against the task's criteria; Test runs the suites of the files the task changed and takes the criteria's shots. "
                              "Command and Land are stages Zerus runs itself."), c.muted), 1);
    stageColumn->addLayout(addRow);
    for (const auto &problem : problems) stageColumn->addWidget(warningLine(problem, c.warn, "teamPipelineProblem"));
    bool stagesEdited = false;
    if (orig) {
        QJsonArray a, b;
        for (const auto &s : p.stages) a.append(s.json());
        for (const auto &s : orig->stages) b.append(s.json());
        stagesEdited = a != b || orig->folder != p.folder;
    }
    add(tr("Stages"), stagesEdited, box(stageColumn));
    // Final acceptance and the rounds.
    auto *acceptRow = row(10);
    auto *accept = segmented({{"lead", tr("Lead")}, {"sub_lead", tr("Sub-lead")}, {"auto", tr("Auto")}}, acceptKind(p), "teamPipelineAccept", [this, at](const QString &value) {
        auto *x = at(); if (!x || acceptKind(*x) == value) return;
        x->accept = value; changed(); rebuildEditor();
    });
    accept->setEnabled(!m_readOnly);
    acceptRow->addWidget(accept);
    const bool endsInLand = !p.stages.isEmpty() && p.stages.last().kind == "land";
    acceptRow->addWidget(hint(endsInLand ? tr("The land stage decides itself: its exit codes accept the task, return it or hold it for the lead.")
                                         : tr("Sub-lead: the member's reviewer_of lead. Auto accepts once the last stage passes."), c.muted), 1);
    add(tr("Final acceptance"), orig && acceptKind(*orig) != acceptKind(p), box(acceptRow));
    auto *roundsRow = row(8);
    roundsRow->addWidget(label(tr("A stage that returns the task"), c.text2, 12));
    auto *rounds = new Stepper(p.rounds(), 1, 10, "teamPipelineRounds", [this, at](int value) { if (auto *x = at()) { x->maxRounds = value; changed(); } });
    rounds->setEnabled(!m_readOnly);
    roundsRow->addWidget(rounds);
    roundsRow->addWidget(hint(tr("times sends it to the lead with every round's findings."), c.text2), 1);
    add(tr("After the rounds"), orig && orig->rounds() != p.rounds(), box(roundsRow));
    outer->addWidget(form);
    outer->addWidget(label(tr("DEFAULTS"), c.muted, 11, true, "teamPipelineDefaultsTitle"));
    outer->addWidget(pipelineDefaults(p));
    outer->addWidget(pipelineTry(p));
    return w;
}

// The open stage's details: where a test works and who takes a stage, or
// what Zerus runs for a command or land stage and a land stage's exit codes.
QWidget *TeamCatalogEditor::pipelineStageDetails(int index, int stage)
{
    const Colors c = colorsFor(m_dark);
    const QString id = m_catalog.pipelines[index].id;
    const Pipeline &p = m_catalog.pipelines[index];
    const PipelineStage &s = p.stages[stage];
    const auto at = [this, id, stage]() -> PipelineStage * {
        for (auto &x : m_catalog.pipelines) if (x.id == id && stage < x.stages.size()) return &x.stages[stage];
        return nullptr;
    };
    auto *frame = new QFrame; frame->setObjectName("teamPipelineStageDetails"); frame->setProperty("stage", stage);
    auto *v = new QVBoxLayout(frame); v->setContentsMargins(14, 10, 14, 12); v->setSpacing(6);
    const auto heading = [&](const QString &text) { v->addWidget(label(text, c.muted, 11, true)); };
    if (s.kind == "test") {
        heading(tr("WHERE THE TEST WORKS"));
        const QString folder = p.folder.isEmpty() ? QStringLiteral("hold") : p.folder;
        const QList<std::tuple<QString, QString, QString, const char *>> options{
            {"hold", tr("The task's folder"),
             tr("The implementer's worktree or clone, or the shared folder when the team works in one. The stage reads and builds there and writes only build output "
                "and its own temporary folder. While it runs the folder is held: the implementer's next task there waits, and its card says “folder held by the "
                "test stage of #N”. In a shared folder the others keep working; the hand-off names the task's files."), "teamPipelineFolderHold"},
            {"checkout", tr("A detached checkout of the attempt's commits"),
             tr("Frees the implementer at once and costs one build. Needs a Git checkout; for a non-Git folder the stage uses the task's folder."), "teamPipelineFolderCheckout"}};
        for (const auto &[value, title, text, name] : options) {
            auto *radio = new QRadioButton(title); radio->setObjectName(name); radio->setChecked(folder == value); radio->setEnabled(!m_readOnly);
            QFont f = radio->font(); f.setWeight(QFont::DemiBold); radio->setFont(f);
            radio->setStyleSheet(QStringLiteral("QRadioButton{color:%1;font-size:13px;}"
                                                "QRadioButton::indicator{width:10px;height:10px;border-radius:7px;border:2px solid %2;background:transparent;}"
                                                "QRadioButton::indicator:checked{border:4px solid %3;width:6px;height:6px;background:%4;}")
                                     .arg(c.fg.name(), c.muted.name(), c.accent.name(), c.cardFill.name()));
            connect(radio, &QRadioButton::clicked, this, [this, id, value = value] {
                for (auto &x : m_catalog.pipelines) if (x.id == id && x.folder != value) { x.folder = value; changed(); rebuildEditor(); }
            });
            v->addWidget(radio);
            auto *about = hint(text, c.text2); about->setContentsMargins(24, 0, 0, 4); v->addWidget(about);
        }
    }
    if (!s.runByZerus()) {
        auto *whoRow = row(10);
        whoRow->addWidget(label(tr("Who takes it"), c.text2, 12));
        const bool pinned = s.role.isEmpty();
        auto *who = segmented({{"role", tr("Role")}, {"member", tr("Pinned member")}}, pinned ? QStringLiteral("member") : QStringLiteral("role"), "teamPipelineStageWho",
                              [this, at](const QString &value) {
            auto *x = at(); if (!x || (value == "member") == x->role.isEmpty()) return;
            if (value == "member") x->role.clear();
            else { x->member.clear(); x->role = defaultStageRole(m_catalog, x->kind); }
            changed(); rebuildEditor();
        });
        who->setEnabled(!m_readOnly);
        whoRow->addWidget(who);
        if (pinned) {
            auto *member = new QLineEdit(s.member); member->setObjectName("teamPipelineStageMember"); member->setReadOnly(m_readOnly);
            member->setPlaceholderText(tr("member id, such as backend2")); member->setMaxLength(40);
            static const QRegularExpression shape(QStringLiteral("[A-Za-z0-9_-]{0,40}"));
            member->setValidator(new QRegularExpressionValidator(shape, member));
            connect(member, &QLineEdit::editingFinished, this, [this, at, member] {
                auto *x = at(); if (!x || member->text().trimmed() == x->member) return;
                x->member = member->text().trimmed(); changed(); rebuildEditor();
            });
            whoRow->addWidget(member, 1);
        } else whoRow->addStretch();
        v->addLayout(whoRow);
        v->addWidget(hint(pinned ? tr("Only this member takes the stage; it waits while the member is busy. The implementer never takes its own task's stage.")
                                 : tr("A second member with the role takes waiting stages. The implementer never takes its own task's stage."), c.muted));
        return frame;
    }
    // U47/U48: the command Zerus runs, its expected exit and time limit.
    heading(s.kind == "land" ? tr("WHAT ZERUS RUNS TO LAND THE TASK") : tr("WHAT ZERUS RUNS"));
    auto *command = new QLineEdit(s.command); command->setObjectName("teamPipelineStageCommand"); command->setReadOnly(m_readOnly);
    command->setFont(NewSessionDense::mono(12)); command->setMaxLength(2000);
    command->setPlaceholderText(s.kind == "land" ? tr("such as scripts/land.sh --squash") : tr("such as cargo test --locked"));
    connect(command, &QLineEdit::editingFinished, this, [this, at, command] {
        auto *x = at(); if (!x || command->text().trimmed() == x->command) return;
        x->command = command->text().trimmed(); x->hasCommand = true; changed(); rebuildEditor();
    });
    v->addWidget(command);
    auto *limits = row(8);
    if (s.kind == "command") {
        limits->addWidget(label(tr("Passes on exit"), c.text2, 12));
        auto *expect = new QSpinBox; expect->setObjectName("teamPipelineStageExpect"); expect->setRange(-255, 255); expect->setValue(s.expectExit);
        expect->setEnabled(!m_readOnly);
        connect(expect, &QSpinBox::editingFinished, this, [this, at, expect] {
            auto *x = at(); if (!x || expect->value() == x->expectExit) return;
            x->expectExit = expect->value(); x->hasCommand = true; changed();
        });
        limits->addWidget(expect); limits->addSpacing(12);
    }
    limits->addWidget(label(tr("Time limit"), c.text2, 12));
    auto *timeout = new QSpinBox; timeout->setObjectName("teamPipelineStageTimeout"); timeout->setRange(1, 240); timeout->setValue(qBound(1, s.timeoutMinutes, 240));
    timeout->setSuffix(tr(" min")); timeout->setEnabled(!m_readOnly);
    connect(timeout, &QSpinBox::editingFinished, this, [this, at, timeout] {
        auto *x = at(); if (!x || timeout->value() == x->timeoutMinutes) return;
        x->timeoutMinutes = timeout->value(); x->hasCommand = true; changed();
    });
    limits->addWidget(timeout); limits->addStretch();
    v->addLayout(limits);
    v->addWidget(hint(s.kind == "land"
                          ? tr("Zerus runs it in the task's folder once the earlier stages pass, as a detached run with no model. Only the owner's catalog names the command; a plan chooses the preset, never the command.")
                          : tr("Zerus runs it in the task's folder as a detached run (hgs run), with no model: the exit code is the verdict, and a return gives the implementer the output's tail as the finding. Only the owner's catalog names the command."),
                      c.muted));
    if (s.kind != "land") return frame;
    heading(tr("EXIT CODES"));
    int k = 0;
    for (auto it = s.exits.cbegin(); it != s.exits.cend(); ++it, ++k) {
        auto *exitRow = row(8);
        exitRow->addWidget(label(tr("Exit"), c.text2, 12));
        auto *code = new QSpinBox; code->setObjectName("teamPipelineExitCode"); code->setRange(-255, 255); code->setValue(it.key().toInt()); code->setEnabled(!m_readOnly);
        auto *outcome = choice({{"accepted", outcomeName("accepted")}, {"returned", outcomeName("returned")}, {"held", outcomeName("held")}}, it.value(), "teamPipelineExitOutcome");
        outcome->setEnabled(!m_readOnly);
        auto *drop = iconButton(workspaceIcon("trash", c.muted), tr("Remove this exit code"), "teamPipelineExitDelete"); drop->setEnabled(!m_readOnly);
        const QString key = it.key();
        connect(code, &QSpinBox::editingFinished, this, [this, at, code, key] {
            auto *x = at(); const QString next = QString::number(code->value());
            if (!x || next == key || x->exits.contains(next)) { if (x && next != key) code->setValue(key.toInt()); return; }
            x->exits.insert(next, x->exits.take(key)); changed(); rebuildEditor();
        });
        connect(outcome, &QComboBox::activated, this, [this, at, outcome, key] { if (auto *x = at()) { x->exits[key] = outcome->currentData().toString(); changed(); } });
        connect(drop, &QToolButton::clicked, this, [this, at, key] { if (auto *x = at()) { x->exits.remove(key); changed(); rebuildEditor(); } });
        exitRow->addWidget(code); exitRow->addWidget(outcome, 1); exitRow->addWidget(drop);
        v->addLayout(exitRow);
    }
    auto *addExit = button(tr("+ Exit code"), "teamPipelineAddExit"); addExit->setProperty("ghost", true); addExit->setEnabled(!m_readOnly);
    connect(addExit, &QPushButton::clicked, this, [this, at] {
        auto *x = at(); if (!x) return;
        int code = x->exits.isEmpty() ? 0 : 1;
        while (x->exits.contains(QString::number(code))) ++code;
        x->exits.insert(QString::number(code), code == 0 ? QStringLiteral("accepted") : QStringLiteral("returned"));
        changed(); rebuildEditor();
    });
    auto *exitsRow = row(10);
    exitsRow->addWidget(addExit);
    exitsRow->addWidget(hint(s.exits.isEmpty() ? tr("None named: exit 0 accepts the task and any other exit holds it for the lead.")
                                               : tr("Any other exit holds the task for the lead."), c.muted), 1);
    v->addLayout(exitsRow);
    // U48: the owner's land_allowed gates it.
    v->addWidget(warningLine(tr("A land stage runs only in a team whose owner turned on land_allowed (Landing in the team's Settings). Without it the plan refuses this preset, "
                                "and a land stage reached anyway is held for the lead, never run."), c.warn, "teamPipelineLandRule"));
    return frame;
}

// Below the editor: the default pipeline per template and per role (a role's
// wins). Each is an edit of that template or role, saved with the catalog.
QWidget *TeamCatalogEditor::pipelineDefaults(const Teams::Pipeline &pipeline)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QWidget; w->setObjectName("teamPipelineDefaults");
    auto *h = new QHBoxLayout(w); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(18);
    QList<std::pair<QString, QString>> presets;
    for (const auto &p : m_catalog.pipelines) presets.append({p.id, p.name});
    const Catalog &base = m_teams->catalog();
    auto *templates = new QGridLayout; templates->setHorizontalSpacing(10); templates->setVerticalSpacing(6);
    templates->addWidget(label(tr("Template"), c.muted, 12), 0, 0); templates->addWidget(label(tr("Pipeline"), c.muted, 12), 0, 1);
    for (int i = 0; i < m_catalog.templates.size(); ++i) {
        const Template &t = m_catalog.templates[i];
        const Template *saved = m_readOnly ? nullptr : base.templ(t.id);
        auto items = presets; items.prepend({QString(), tr("None (today's flow)")});
        auto *picker = choice(items, t.pipeline, "teamPipelineTemplateDefault"); picker->setProperty("template", t.id); picker->setEnabled(!m_readOnly);
        connect(picker, &QComboBox::activated, this, [this, i, picker] {
            if (m_catalog.templates[i].pipeline == picker->currentData().toString()) return;
            m_catalog.templates[i].pipeline = picker->currentData().toString(); changed(); rebuildEditor();
        });
        auto *nameRow = row(4); nameRow->addWidget(ellipsis(t.name, c.fg, 13), 1);
        if (saved && saved->pipeline != t.pipeline) nameRow->addWidget(label(QStringLiteral("●"), c.warn, 9));
        templates->addWidget(box(nameRow), i + 1, 0); templates->addWidget(picker, i + 1, 1);
    }
    templates->setColumnStretch(0, 1); templates->setColumnStretch(1, 1);
    auto *roles = new QGridLayout; roles->setHorizontalSpacing(10); roles->setVerticalSpacing(6);
    roles->addWidget(label(tr("Role with its own"), c.muted, 12), 0, 0); roles->addWidget(label(tr("Pipeline"), c.muted, 12), 0, 1);
    int at = 1;
    QList<std::pair<QString, QString>> without;
    for (int i = 0; i < m_catalog.roles.size(); ++i) {
        const Role &r = m_catalog.roles[i];
        if (r.pipeline.isEmpty()) { without.append({r.id, r.name}); continue; }
        auto items = presets; items.prepend({QString(), tr("The template's default")}); items.append({QStringLiteral("none"), tr("None (today's flow)")});
        auto *picker = choice(items, r.pipeline, "teamPipelineRoleDefault"); picker->setProperty("role", r.id); picker->setEnabled(!m_readOnly);
        connect(picker, &QComboBox::activated, this, [this, i, picker] {
            if (m_catalog.roles[i].pipeline == picker->currentData().toString()) return;
            m_catalog.roles[i].pipeline = picker->currentData().toString(); changed(); rebuildEditor();
        });
        auto *chipBox = row(4); chipBox->addWidget(chipOf(r, m_dark));
        if (const Role *saved = m_readOnly ? nullptr : base.role(r.id); saved && saved->pipeline != r.pipeline) chipBox->addWidget(label(QStringLiteral("●"), c.warn, 9));
        chipBox->addStretch();
        roles->addWidget(box(chipBox), at, 0); roles->addWidget(picker, at++, 1);
    }
    // + Role: the role then uses this preset.
    without.prepend({QString(), tr("+ Role")});
    auto *addRole = choice(without, QString(), "teamPipelineAddRole");
    addRole->setEnabled(!m_readOnly && without.size() > 1);
    addRole->setToolTip(tr("Give a role its own pipeline: %1").arg(pipeline.name));
    connect(addRole, &QComboBox::activated, this, [this, addRole, id = pipeline.id] {
        const QString role = addRole->currentData().toString();
        for (auto &r : m_catalog.roles) if (r.id == role && !role.isEmpty()) { r.pipeline = id; changed(); rebuildEditor(); return; }
    });
    roles->addWidget(addRole, at++, 0, Qt::AlignLeft);
    roles->addWidget(hint(tr("A role's pipeline wins over its template's. The plan and edit-draft can set any task's; a task keeps a snapshot from its "
                             "assignment, so later edits here do not change a running task."), c.muted), at, 0, 1, 2);
    roles->setColumnStretch(0, 1); roles->setColumnStretch(1, 1); roles->setRowStretch(at + 1, 1);
    h->addLayout(templates, 1); h->addLayout(roles, 1);
    return w;
}

// A designer's change to a preset: each field it changes, old → new.
QWidget *TeamCatalogEditor::pipelineDiff(const Teams::Pipeline &mine, const Teams::Proposal &proposal)
{
    const Pipeline *saved = m_teams->catalog().pipeline(proposal.target);
    if (!saved || proposal.action != "change") return nullptr;
    const Colors c = colorsFor(m_dark);
    const auto flow = [this](const Pipeline &p) {
        QStringList parts;
        for (const auto &s : p.stages) {
            const Role *r = m_catalog.role(s.role);
            parts << (s.runByZerus() ? stageName(s.kind) : tr("%1 by %2").arg(stageName(s.kind), !s.member.isEmpty() ? s.member : r ? r->name : s.role));
        }
        return parts.isEmpty() ? tr("no stages") : parts.join(QStringLiteral(" › "));
    };
    QList<std::tuple<QString, QString, QString>> fields{
        {tr("Name"), saved->name, mine.name}, {tr("Stages"), flow(*saved), flow(mine)},
        {tr("Test works in"), folderName(saved->folder), folderName(mine.folder)},
        {tr("Final acceptance"), stageName(acceptKind(*saved)), stageName(acceptKind(mine))},
        {tr("Rounds"), QString::number(saved->rounds()), QString::number(mine.rounds())}};
    auto *diff = new QFrame; diff->setObjectName("teamPipelineDiff");
    auto *v = new QVBoxLayout(diff); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(3);
    const bool rejected = proposal.state == "rejected";
    for (const auto &[name, before, after] : fields) {
        if (before == after) continue;
        auto *l = new QLabel(QStringLiteral("<span style='color:%1;'>%2:</span> <s style='color:%3;'>%4</s> <span style='color:%1;'>→</span> <span style='color:%5;'>%6</span>")
                                 .arg(c.muted.name(), name.toHtmlEscaped(), (rejected ? c.dim : c.danger).name(), before.toHtmlEscaped(),
                                      (rejected ? c.dim : c.accent).name(), after.toHtmlEscaped()));
        l->setObjectName("teamPipelineDiffLine"); l->setTextFormat(Qt::RichText); l->setWordWrap(true);
        l->setStyleSheet(QStringLiteral("font-size:12px;"));
        v->addWidget(l);
    }
    if (v->count()) return diff;
    delete diff;
    return nullptr;
}

// Try it: a task of a running team (or a sample team of one member per role)
// walked through the preset; nothing is sent.
QWidget *TeamCatalogEditor::pipelineTry(const Teams::Pipeline &pipeline)
{
    const Colors c = colorsFor(m_dark);
    auto *frame = new QFrame; frame->setObjectName("teamPipelineTryBox");
    auto *v = new QVBoxLayout(frame); v->setContentsMargins(14, 12, 14, 14); v->setSpacing(10);
    auto *head = row(8);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("check-circle", c.accent).pixmap(15, 15)); icon->setFixedSize(16, 16);
    auto *title = new QLabel(QStringLiteral("<b>%1</b> <span style='color:%2;'>%3</span>").arg(tr("Try it").toHtmlEscaped(), c.muted.name(), tr("on a sample task — nothing is sent").toHtmlEscaped()));
    title->setTextFormat(Qt::RichText); title->setStyleSheet(QStringLiteral("color:%1;font-size:13px;").arg(c.fg.name()));
    head->addWidget(icon); head->addWidget(title);
    auto *sample = new QComboBox; sample->setObjectName("teamPipelineTrySample");
    sample->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon); sample->setMinimumContentsLength(18);
    int shown = 0;
    for (const auto &team : m_teams->teams()) {
        for (const auto &task : team.tasks) {
            if (shown >= 40 || task.status == "draft" || task.status == "cancelled") continue;
            const Member *m = nullptr; for (const auto &x : team.members) if (x.id == task.assignee) m = &x;
            const QString role = m ? (m->role.isEmpty() ? m->id : m->role) : task.role;
            sample->addItem(QStringLiteral("#%1 %2 · %3 · %4").arg(task.id).arg(task.title, role, team.name), QStringLiteral("%1/%2").arg(team.id).arg(task.id));
            ++shown;
        }
    }
    if (!sample->count()) sample->addItem(tr("A sample task, one member per role"), QStringLiteral("sample"));
    if (const int found = sample->findData(property("pipelineTry").toString()); found >= 0) sample->setCurrentIndex(found);
    auto *go = button(tr("Try"), "teamPipelineTry", true);
    head->addWidget(sample, 1); head->addWidget(go);
    v->addLayout(head);
    auto *result = new QWidget; result->setObjectName("teamPipelineTryResult");
    auto *rv = new QVBoxLayout(result); rv->setContentsMargins(0, 0, 0, 0); rv->setSpacing(8);
    v->addWidget(result);
    const auto show = [this, rv, c, pipeline](const QString &key) {
        while (auto *item = rv->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
        // The team and task the key names, or one member per role.
        Team team; Task task;
        bool found = false;
        for (const auto &t : m_teams->teams())
            for (const auto &x : t.tasks) if (QStringLiteral("%1/%2").arg(t.id).arg(x.id) == key) { team = t; task = x; found = true; }
        if (!found) {
            team.name = tr("Sample team");
            for (const auto &r : m_catalog.roles) {
                Member m; m.id = r.id; m.role = r.name; m.agent = r.agent; m.model = r.model; m.lead = r.id == "lead";
                team.members.append(m);
            }
            task.id = 1; task.title = tr("Sample task");
            QStringList staged; for (const auto &s : pipeline.stages) staged << s.role;
            for (const auto &m : team.members) if (!m.lead && task.assignee.isEmpty() && !staged.contains(m.id)) task.assignee = m.id;
            for (const auto &m : team.members) if (!m.lead && task.assignee.isEmpty()) task.assignee = m.id;
        }
        const Walk walked = walk(m_catalog, pipeline, team, task);
        for (int i = 0; i < walked.steps.size(); ++i) {
            const Step &step = walked.steps[i];
            auto *stepRow = new QWidget; stepRow->setObjectName("teamPipelineTryStep"); stepRow->setProperty("kind", step.kind); stepRow->setProperty("member", step.member);
            auto *h = new QHBoxLayout(stepRow); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(10);
            h->addWidget(new StageChip(step.kind, QString(), stageColor(step.kind, c)), 0, Qt::AlignTop);
            auto *text = new QVBoxLayout; text->setSpacing(2);
            auto *first = row(6);
            first->addWidget(label(step.title, c.fg, 13, true));
            if (const Role *r = m_catalog.roleNamed(step.role)) first->addWidget(chipOf(*r, m_dark));
            else if (!step.role.isEmpty()) first->addWidget(label(step.role, c.text2, 12));
            if (!step.agent.isEmpty()) { auto *logo = new QLabel; logo->setPixmap(agentLogo(step.agent, c.fg).pixmap(14, 14)); logo->setToolTip(agentNames.value(step.agent, step.agent)); first->addWidget(logo); }
            if (!step.detail.isEmpty()) first->addWidget(ellipsis(step.detail, c.muted, 12), 1); else first->addStretch();
            text->addLayout(first);
            if (!step.where.isEmpty()) text->addWidget(hint(step.where, c.text2));
            h->addLayout(text, 1);
            rv->addWidget(stepRow);
        }
        for (const auto &warning : walked.warnings) rv->addWidget(warningLine(warning, c.warn, "teamPipelineTryWarning"));
    };
    connect(go, &QPushButton::clicked, this, [this, sample, show] { setProperty("pipelineTry", sample->currentData().toString()); show(sample->currentData().toString()); });
    // After an edit the walk follows the preset as it is now.
    if (!property("pipelineTry").toString().isEmpty()) show(sample->currentData().toString());
    return frame;
}

TeamCatalogEditor::Walk TeamCatalogEditor::walk(const Teams::Catalog &catalog, const Teams::Pipeline &pipeline, const Teams::Team &team, const Teams::Task &task)
{
    Walk out;
    const auto roleName = [&](const QString &role) { const Role *r = catalog.roleNamed(role); return r ? r->name : role; };
    const auto hasRole = [&](const Member &m, const QString &role) {
        const Role *r = catalog.roleNamed(m.role);
        return m.role.compare(role, Qt::CaseInsensitive) == 0 || (r && r->id == role);
    };
    const Member *implementer = nullptr, *lead = nullptr;
    for (const auto &m : team.members) { if (m.id == task.assignee) implementer = &m; if (m.lead && !lead) lead = &m; }
    if (!implementer && !task.role.isEmpty()) for (const auto &m : team.members) if (!implementer && !m.lead && hasRole(m, task.role)) implementer = &m;
    const QString folder = implementer && !implementer->directory.isEmpty() ? implementer->directory : team.folder;
    const QString where = folder.isEmpty() ? tr("the task's folder") : folder;
    const QString who = implementer ? roleName(implementer->role.isEmpty() ? implementer->id : implementer->role) : tr("The implementer");
    const auto describe = [&](const Member &m) {
        QStringList parts;
        if (!m.model.isEmpty()) parts << m.model;
        if (!m.computer.isEmpty()) parts << m.computer;
        const auto queue = memberQueue(team, m.id);
        parts << (queue.isEmpty() ? tr("free now") : queue.size() == 1 ? tr("busy with #%1").arg(queue.first())
                                                   : tr("busy with #%1, %2 waiting").arg(queue.first()).arg(queue.size() - 1));
        return parts.join(QStringLiteral(" · "));
    };
    Step implement{QStringLiteral("implement"), tr("Implement"), implementer ? implementer->role : task.role, implementer ? implementer->id : QString(),
                   implementer ? implementer->agent : QString(), implementer ? describe(*implementer) : tr("no member yet"),
                   folder.isEmpty() ? tr("In its own folder; reports with its commits.") : tr("In its folder %1; reports with its commits.").arg(folder)};
    out.steps << implement;
    const int retry = int(qMax<qsizetype>(1, task.attempts.size())) + 1;
    const QString rounds = tr("up to %1 rounds").arg(pipeline.rounds());
    bool landed = false;
    for (int i = 0; i < pipeline.stages.size(); ++i) {
        const PipelineStage &s = pipeline.stages[i];
        Step step; step.kind = s.kind;
        if (s.runByZerus()) {
            step.role = tr("Zerus");
            step.detail = s.command.trimmed().isEmpty() ? tr("no command yet") : s.command.trimmed();
            if (s.command.trimmed().isEmpty()) out.warnings << tr("Stage %1 has no command yet: the catalog cannot be saved without one.").arg(i + 1);
            if (s.kind == "command") {
                step.title = tr("Command, %1").arg(rounds);
                step.where = tr("Zerus runs it in %1 with no model: exit %2 within %3 min passes, any other exit returns the task to %4 with the output's tail.")
                                 .arg(where).arg(s.expectExit).arg(s.timeoutMinutes).arg(who);
            } else {
                landed = true;
                step.title = tr("Land");
                QStringList codes;
                for (auto it = s.exits.cbegin(); it != s.exits.cend(); ++it) codes << tr("exit %1: %2").arg(it.key(), outcomeName(it.value()).toLower());
                step.where = tr("Zerus runs it in %1 once the earlier stages pass; %2.")
                                 .arg(where, codes.isEmpty() ? tr("exit 0 accepts the task, any other holds it for the lead") : codes.join(QStringLiteral(", ")) + tr(", any other exit holds it for the lead"));
                if (!team.settingsJson.value("land_allowed").toBool())
                    out.warnings << tr("This team does not allow landing (land_allowed is off): the plan refuses this preset, and a land stage reached anyway is held for the lead.");
            }
            out.steps << step;
            continue;
        }
        step.title = tr("%1, %2").arg(stageName(s.kind), rounds);
        QList<const Member *> candidates;
        bool own = false;
        for (const auto &m : team.members) {
            const bool takes = s.member.isEmpty() ? !m.draining && hasRole(m, s.role) : m.id == s.member;
            if (!takes) continue;
            if (&m == implementer) own = true; else candidates << &m;
        }
        step.role = s.member.isEmpty() ? s.role : s.member;
        // The first free one, else the one with the shortest queue.
        const Member *pick = nullptr;
        qsizetype least = 0;
        for (const auto *m : candidates) {
            const qsizetype load = memberQueue(team, m->id).size();
            if (!pick || load < least) { pick = m; least = load; }
        }
        if (pick) { step.member = pick->id; step.agent = pick->agent; step.detail = describe(*pick); }
        else if (!s.member.isEmpty()) {
            step.detail = own ? tr("the implementer: it never takes its own task's stage") : tr("not in this team");
            out.warnings << (own ? tr("Stage %1 is pinned to %2, the implementer of this task: it waits for the lead.").arg(i + 1).arg(s.member)
                                 : tr("Stage %1 is pinned to %2, who is not in this team: it waits.").arg(i + 1).arg(s.member));
        } else {
            step.detail = tr("no other %1 in this team").arg(roleName(s.role));
            out.warnings << tr("No %1 besides the implementer: stage %2 waits until one joins.").arg(roleName(s.role)).arg(i + 1);
        }
        if (s.kind == "review") step.where = tr("Reads %1 at the attempt's commits. Return → %2 gets the findings as attempt %3.").arg(where, who).arg(retry);
        else if (pipeline.folder == "checkout") step.where = tr("Builds a detached checkout of the attempt's commits and runs the suites of the files the task changed; %1 is free while it runs.").arg(who);
        else step.where = tr("Builds in %1 and runs the suites of the files the task changed. Holds the folder: %2's next task there waits.").arg(where, who);
        if (s.kind == "test" && s.member.isEmpty() && candidates.size() == 1)
            out.warnings << (pipeline.folder == "checkout"
                                 ? tr("One %1 in this team: test stages queue behind it. A second %1 shortens the wait.").arg(roleName(s.role))
                                 : tr("One %1 in this team: test stages queue behind it. A second %1, or a detached checkout, shortens the hold on %2.").arg(roleName(s.role), who));
        out.steps << step;
    }
    if (!landed) {
        Step accept; accept.kind = acceptKind(pipeline);
        if (accept.kind == "auto") { accept.title = tr("Zerus accepts"); accept.where = tr("Once the last stage passes, with no lead."); }
        else if (accept.kind == "sub_lead") { accept.title = tr("Sub-lead accepts"); accept.where = tr("The implementer's reviewer_of lead decides; without one, the lead."); }
        else {
            accept.title = tr("Lead accepts");
            if (lead) { accept.role = lead->role.isEmpty() ? QStringLiteral("lead") : lead->role; accept.member = lead->id; accept.agent = lead->agent; }
        }
        if (!pipeline.stages.isEmpty())
            accept.where = (accept.where.isEmpty() ? QString() : accept.where + QLatin1Char(' '))
                           + tr("After %1 returned rounds in one stage the task goes to the lead with every round's findings.").arg(pipeline.rounds());
        out.steps << accept;
    }
    out.warnings.removeDuplicates();
    return out;
}
