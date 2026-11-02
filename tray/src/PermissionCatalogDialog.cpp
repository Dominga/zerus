#include "PermissionCatalogDialog.h"
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFont>
#include <QHBoxLayout>
#include <QLabel>
#include <QPushButton>
#include <QSignalBlocker>
#include <QVBoxLayout>
#include <algorithm>

namespace {
QLabel *mutedLabel(const QString &text)
{
    auto *label = new QLabel(text);
    label->setTextFormat(Qt::PlainText);
    label->setWordWrap(true);
    label->setObjectName(QStringLiteral("muted"));
    return label;
}
} // namespace

PermissionCatalogDialog::PermissionCatalogDialog(const PermissionCatalog::Catalog &catalog,
                                                 const QMap<QString, QString> &selection,
                                                 const QString &subtitle,
                                                 QWidget *parent)
    : QDialog(parent), m_catalog(catalog), m_selection(selection)
{
    setObjectName(QStringLiteral("nativePermissionEditor"));
    setWindowTitle(tr("Native permissions"));
    setMinimumWidth(520);
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(18, 18, 18, 18);
    layout->setSpacing(12);
    layout->setSizeConstraint(QLayout::SetMinimumSize);
    if (!subtitle.isEmpty()) layout->addWidget(mutedLabel(subtitle));

    m_warning = new QLabel;
    m_warning->setObjectName(QStringLiteral("nativePermissionWarning"));
    m_warning->setWordWrap(true);
    m_warning->setStyleSheet(QStringLiteral("color:#b45309;"));
    m_warning->hide();
    layout->addWidget(m_warning);

    if (catalog.malformed) {
        auto *reason = mutedLabel(tr("The saved native permission settings cannot be read. Use %1 settings to reset them, then choose again.").arg(catalog.harnessName));
        layout->addWidget(reason);
        auto *reset = new QPushButton(PermissionCatalog::harnessSettingsLabel(catalog));
        reset->setObjectName(QStringLiteral("nativePermissionReset"));
        connect(reset, &QPushButton::clicked, this, [this] {
            m_selection.clear();
            accept();
        });
        layout->addWidget(reset, 0, Qt::AlignLeft);
    } else {
        for (const auto &row : PermissionCatalog::editorRows(catalog, selection)) {
            if (auto *widget = addRow(row)) layout->addWidget(widget);
        }
    }

    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel);
    m_save = buttons->button(QDialogButtonBox::Save);
    m_save->setObjectName(QStringLiteral("nativePermissionSave"));
    connect(buttons, &QDialogButtonBox::accepted, this, [this] {
        if (m_save->isEnabled()) accept();
    });
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    layout->addWidget(buttons);
    validate();
}

QWidget *PermissionCatalogDialog::addRow(const PermissionCatalog::EditorRow &row)
{
    auto *container = new QWidget;
    container->setObjectName(QStringLiteral("nativePermissionRow_") + row.id);
    auto *layout = new QVBoxLayout(container);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(4);
    auto *name = new QLabel(row.label);
    name->setTextFormat(Qt::PlainText);
    name->setWordWrap(true);
    QFont font = name->font();
    font.setWeight(QFont::DemiBold);
    name->setFont(font);
    name->setObjectName(QStringLiteral("nativePermissionLabel_") + row.id);
    layout->addWidget(name);
    if (!row.description.isEmpty()) layout->addWidget(mutedLabel(row.description));

    if (row.unknown) {
        auto *line = new QHBoxLayout;
        auto *value = new QLabel(row.selected);
        value->setTextFormat(Qt::PlainText);
        value->setTextInteractionFlags(Qt::TextSelectableByMouse);
        value->setObjectName(QStringLiteral("nativePermissionValue_") + row.id);
        line->addWidget(value, 1);
        auto *remove = new QPushButton(tr("Remove"));
        remove->setObjectName(QStringLiteral("nativePermissionRemove_") + row.id);
        connect(remove, &QPushButton::clicked, this, [this, id = row.id] { removeUnknown(id); });
        line->addWidget(remove);
        layout->addLayout(line);
        m_unknown.append({row.id, container, remove});
        return container;
    }

    auto *combo = new QComboBox;
    combo->setObjectName(QStringLiteral("nativePermission_") + row.id);
    combo->setAccessibleName(row.label);
    combo->setSizeAdjustPolicy(QComboBox::AdjustToContents);
    combo->addItem(PermissionCatalog::harnessSettingsLabel(m_catalog), QString());
    for (const auto &option : row.options) {
        combo->addItem(option.label, option.value);
        combo->setItemData(combo->count() - 1, option.description, Qt::ToolTipRole);
    }
    if (row.unavailable) {
        combo->addItem(tr("Unavailable: %1").arg(row.selected), row.selected);
        combo->setItemData(combo->count() - 1, tr("This saved value is no longer offered."), Qt::ToolTipRole);
    }
    const int index = row.selected.isEmpty() ? 0 : combo->findData(row.selected);
    combo->setCurrentIndex(index < 0 ? 0 : index);
    if (row.unavailable) combo->setCurrentIndex(combo->count() - 1);
    connect(combo, &QComboBox::currentIndexChanged, this, [this, id = row.id, combo] {
        const auto value = combo->currentData().toString();
        if (value.isEmpty()) m_selection.remove(id);
        else m_selection.insert(id, value);
        validate();
    });
    layout->addWidget(combo);
    return container;
}

void PermissionCatalogDialog::removeUnknown(const QString &id)
{
    m_selection.remove(id);
    for (auto &unknown : m_unknown) {
        if (unknown.id != id) continue;
        if (unknown.widget) unknown.widget->hide();
        if (unknown.remove) unknown.remove->setEnabled(false);
    }
    validate();
}

void PermissionCatalogDialog::validate()
{
    if (!m_save) return;
    const auto problem = PermissionCatalog::selectionProblem(m_catalog, m_selection);
    const bool unknownLeft = std::any_of(m_unknown.cbegin(), m_unknown.cend(), [this](const UnknownRow &row) {
        return m_selection.contains(row.id);
    });
    const bool valid = problem.isEmpty() && !unknownLeft && !m_catalog.malformed;
    m_save->setEnabled(valid);
    if (!valid) {
        m_warning->setText(m_catalog.malformed
            ? tr("Reset the unreadable saved settings before saving.")
            : unknownLeft ? tr("Remove each unknown saved setting before saving.")
            : problem);
        m_warning->show();
    } else {
        m_warning->hide();
    }
}

QString PermissionCatalogDialog::mode() const { return PermissionCatalog::nativeValue(m_selection); }
