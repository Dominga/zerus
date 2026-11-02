#include "PermissionCatalog.h"
#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonValue>
#include <QRegularExpression>

namespace PermissionCatalog {
namespace {
QString translated(const char *text) { return QCoreApplication::translate("PermissionCatalog", text); }

QString bounded(const QString &text)
{
    return text.size() > 80 ? text.left(77) + QStringLiteral("…") : text;
}

QString unavailableValue(const QString &value)
{
    return translated("%1 (unavailable)").arg(bounded(value));
}

bool isBypassText(const QString &value, const QString &label)
{
    return value.contains(QStringLiteral("bypass"), Qt::CaseInsensitive)
        || label.contains(QStringLiteral("bypass"), Qt::CaseInsensitive);
}

const Control *findControl(const Catalog &catalog, const QString &id)
{
    for (const auto &control : catalog.controls)
        if (control.id == id) return &control;
    return nullptr;
}

const Option *findOption(const Control &control, const QString &value)
{
    for (const auto &option : control.options)
        if (option.value == value) return &option;
    return nullptr;
}

void appendUnavailable(QList<Choice> &choices, const Catalog &catalog)
{
    Choice choice;
    choice.label = translated("Unavailable: %1").arg(bounded(catalog.savedMode));
    choice.value = catalog.savedMode;
    choice.tooltip = translated("This saved choice is not offered by the current harness. Choose another option before saving.");
    choice.unavailable = true;
    choices.append(choice);
}
} // namespace

bool validControlId(const QString &id)
{
    static const QRegularExpression pattern(QStringLiteral("^[a-z][a-z0-9_]*$"));
    return pattern.match(id).hasMatch();
}

bool decodeNative(const QString &mode, QMap<QString, QString> *values)
{
    if (!mode.startsWith(QStringLiteral("native:"))) return false;
    // The stored selector is bounded, and only string values of valid control
    // ids are accepted; a malformed map is never shown as the provider default.
    if (mode.toUtf8().size() > 1024) return false;
    const auto document = QJsonDocument::fromJson(mode.mid(7).toUtf8());
    if (!document.isObject()) return false;
    const auto object = document.object();
    QMap<QString, QString> decoded;
    for (auto it = object.begin(); it != object.end(); ++it) {
        if (!validControlId(it.key()) || !it.value().isString() || it.value().toString().isEmpty()) return false;
        decoded.insert(it.key(), it.value().toString());
    }
    if (values) *values = decoded;
    return true;
}

QString nativeValue(const QMap<QString, QString> &values)
{
    QMap<QString, QString> filtered;
    for (auto it = values.begin(); it != values.end(); ++it)
        if (!it.value().isEmpty()) filtered.insert(it.key(), it.value());
    if (filtered.isEmpty()) return QStringLiteral("provider");
    QJsonObject object;
    for (auto it = filtered.begin(); it != filtered.end(); ++it) object.insert(it.key(), it.value());
    return QStringLiteral("native:") + QString::fromUtf8(QJsonDocument(object).toJson(QJsonDocument::Compact));
}

Catalog parse(const QJsonObject &profile)
{
    Catalog catalog;
    catalog.harnessName = profile.value(QStringLiteral("provider")).toString();
    if (catalog.harnessName == QStringLiteral("claude")) catalog.harnessName = QStringLiteral("Claude Code");
    else if (catalog.harnessName == QStringLiteral("codex")) catalog.harnessName = QStringLiteral("Codex");
    else if (catalog.harnessName == QStringLiteral("kimi")) catalog.harnessName = QStringLiteral("Kimi");
    if (catalog.harnessName.isEmpty()) catalog.harnessName = translated("harness");
    catalog.savedMode = profile.value(QStringLiteral("permission_mode")).toString();
    if (catalog.savedMode.isEmpty()) catalog.savedMode = QStringLiteral("provider");
    catalog.nativeSaved = catalog.savedMode.startsWith(QStringLiteral("native:"));

    const auto current = profile.value(QStringLiteral("permission_current")).toObject();
    if (!current.isEmpty()) {
        catalog.currentSource = current.value(QStringLiteral("source")).toString();
        catalog.currentDetail = current.value(QStringLiteral("detail")).toString();
        const auto values = current.value(QStringLiteral("values")).toObject();
        for (auto it = values.begin(); it != values.end(); ++it)
            if (it.value().isString()) catalog.currentValues.insert(it.key(), it.value().toString());
    }
    catalog.effectiveMode = profile.value(QStringLiteral("effective_permission_mode")).toString();
    catalog.legacyDetail = profile.value(QStringLiteral("permission_detail")).toString();

    const auto raw = profile.value(QStringLiteral("permission_catalog")).toObject();
    catalog.present = !raw.isEmpty();
    bool selectedReported = false;
    QMap<QString, QString> reported;
    if (catalog.present) {
        catalog.status = raw.value(QStringLiteral("status")).toString();
        catalog.statusReason = raw.value(QStringLiteral("status_reason")).toString();
        catalog.source = raw.value(QStringLiteral("source")).toString();
        catalog.version = raw.value(QStringLiteral("version")).toString();
        catalog.sessionCapabilityReported = raw.contains(QStringLiteral("session_supported"));
        catalog.sessionSupported = raw.value(QStringLiteral("session_supported")).toBool(false);
        for (const auto &value : raw.value(QStringLiteral("controls")).toArray()) {
            const auto object = value.toObject();
            Control control;
            control.id = object.value(QStringLiteral("id")).toString();
            if (!validControlId(control.id)) continue;
            if (findControl(catalog, control.id)) continue;
            control.label = object.value(QStringLiteral("label")).toString();
            if (control.label.isEmpty()) control.label = control.id;
            control.description = object.value(QStringLiteral("description")).toString();
            for (const auto &entry : object.value(QStringLiteral("conflicts_with")).toArray()) {
                const auto id = entry.toString();
                if (validControlId(id)) control.conflictsWith.append(id);
            }
            for (const auto &entry : object.value(QStringLiteral("options")).toArray()) {
                const auto source = entry.toObject();
                Option option;
                option.value = source.value(QStringLiteral("value")).toString();
                if (option.value.isEmpty()) continue;
                option.label = source.value(QStringLiteral("label")).toString(option.value);
                option.description = source.value(QStringLiteral("description")).toString();
                control.options.append(option);
            }
            catalog.controls.append(control);
        }
        catalog.available = catalog.status == QStringLiteral("available") && !catalog.controls.isEmpty();
        const auto selected = raw.value(QStringLiteral("selected"));
        if (selected.isObject()) {
            selectedReported = true;
            const auto object = selected.toObject();
            for (auto it = object.begin(); it != object.end(); ++it) {
                if (!validControlId(it.key()) || !it.value().isString() || it.value().toString().isEmpty()) {
                    catalog.malformed = true;
                    catalog.selectedValid = false;
                    continue;
                }
                reported.insert(it.key(), it.value().toString());
            }
        }
    }

    if (catalog.nativeSaved) {
        QMap<QString, QString> decoded;
        if (!decodeNative(catalog.savedMode, &decoded)) {
            catalog.malformed = true;
            catalog.selectedValid = false;
        } else if (selectedReported) {
            // A backend-decoded selection is authoritative; validate it like a
            // stored map so only strings ever reach the opaque selector.
            catalog.selected = reported;
        } else {
            catalog.selected = decoded;
        }
    } else if (selectedReported && catalog.present) {
        // A legacy provider/bypass/auto selector is shown as that selector;
        // permission_current carries its observed values separately.
        catalog.selected.clear();
    }

    if (!catalog.malformed) {
        if (catalog.available) {
            for (auto it = catalog.selected.begin(); it != catalog.selected.end(); ++it) {
                if (!validControlId(it.key())) {
                    catalog.unknownKeys.append(it.key());
                    catalog.selectedValid = false;
                    continue;
                }
                const auto *control = findControl(catalog, it.key());
                if (!control) {
                    catalog.unknownKeys.append(it.key());
                    catalog.selectedValid = false;
                    continue;
                }
                if (!findOption(*control, it.value())) {
                    catalog.unavailableControls.append(it.key());
                    catalog.selectedValid = false;
                }
            }
        } else if (catalog.nativeSaved && !catalog.selected.isEmpty()) {
            // Without a catalog the fallback can only preserve the choice.
            catalog.selectedValid = false;
            for (auto it = catalog.selected.begin(); it != catalog.selected.end(); ++it) catalog.unknownKeys.append(it.key());
        }
    }
    catalog.unknownKeys.removeDuplicates();
    catalog.unavailableControls.removeDuplicates();
    if (catalog.available && catalog.selectedValid && !catalog.selected.isEmpty())
        catalog.selectedValid = selectionProblem(catalog, catalog.selected).isEmpty();
    return catalog;
}

QString optionLabel(const Catalog &catalog, const QString &control, const QString &value)
{
    if (const auto *found = findControl(catalog, control))
        if (const auto *option = findOption(*found, value)) return option->label;
    return value;
}

QString displayLabel(const QString &legacyMode)
{
    if (legacyMode.isEmpty() || legacyMode == QStringLiteral("provider") || legacyMode == QStringLiteral("default")
        || legacyMode == QStringLiteral("unknown"))
        return translated("Default");
    if (legacyMode == QStringLiteral("bypass")) return translated("Bypass");
    if (legacyMode == QStringLiteral("auto")) return translated("Auto");
    if (legacyMode == QStringLiteral("plan")) return translated("Plan");
    if (legacyMode == QStringLiteral("mixed")) return translated("Varies");
    return legacyMode;
}

QString summary(const Catalog &catalog, const QMap<QString, QString> &values)
{
    if (catalog.malformed) return unavailableValue(catalog.savedMode);
    if (values.isEmpty()) {
        if (catalog.nativeSaved) return displayLabel(QStringLiteral("provider"));
        if (catalog.savedMode == QStringLiteral("provider")) {
            // A provider selector with an observed current map is still real
            // information when native discovery fails.
            if (!catalog.available && !catalog.currentValues.isEmpty()) {
                QStringList raw;
                for (auto it = catalog.currentValues.begin(); it != catalog.currentValues.end(); ++it)
                    raw.append(bounded(it.key() + QLatin1Char('=') + it.value()));
                return raw.join(QStringLiteral(" · ")) + QLatin1Char(' ') + translated("(provider)");
            }
            if (!catalog.effectiveMode.isEmpty()) return displayLabel(catalog.effectiveMode);
            return displayLabel(QStringLiteral("provider"));
        }
        if (catalog.savedMode == QStringLiteral("auto")) return unavailableValue(translated("Auto"));
        return displayLabel(catalog.savedMode);
    }
    QStringList labels;
    if (catalog.available) {
        for (const auto &control : catalog.controls) {
            if (!values.contains(control.id)) continue;
            const auto *option = findOption(control, values.value(control.id));
            labels.append(option ? option->label : unavailableValue(values.value(control.id)));
        }
        for (auto it = values.begin(); it != values.end(); ++it)
            if (!findControl(catalog, it.key()))
                labels.append(unavailableValue(it.key() + QLatin1Char('=') + it.value()));
        if (labels.isEmpty()) return displayLabel(QStringLiteral("provider"));
        return labels.join(QStringLiteral(" · "));
    }
    // Without a catalog the observed native selector is still meaningful: show
    // its bounded raw values and mark them non-selectable, never as Default.
    QStringList raw;
    for (auto it = values.begin(); it != values.end(); ++it)
        raw.append(bounded(it.key() + QLatin1Char('=') + it.value()));
    return unavailableValue(raw.join(QStringLiteral(" · ")));
}

QString summary(const Catalog &catalog) { return summary(catalog, catalog.selected); }

bool isBypass(const Catalog &catalog)
{
    if (catalog.savedMode == QStringLiteral("bypass")) return true;
    if (catalog.savedMode == QStringLiteral("provider") && catalog.effectiveMode == QStringLiteral("bypass")) return true;
    if (!catalog.selected.isEmpty()) {
        for (const auto &control : catalog.controls)
            if (catalog.selected.contains(control.id))
                if (const auto *option = findOption(control, catalog.selected.value(control.id)))
                    if (isBypassText(option->value, option->label)) return true;
        for (auto it = catalog.selected.begin(); it != catalog.selected.end(); ++it)
            if (isBypassText(it.value(), it.value())) return true;
    }
    return false;
}

QString harnessSettingsLabel(const Catalog &catalog)
{
    return translated("Use %1 settings").arg(catalog.harnessName);
}

QList<Choice> accountChoices(const Catalog &catalog)
{
    QList<Choice> choices;
    Choice provider;
    provider.label = harnessSettingsLabel(catalog);
    provider.value = QStringLiteral("provider");
    provider.tooltip = translated("Use %1's own permission settings instead of a selection saved in Zerus.").arg(catalog.harnessName);
    choices.append(provider);
    Choice bypass;
    bypass.label = translated("Bypass permissions");
    bypass.value = QStringLiteral("bypass");
    bypass.tooltip = translated("Run tools without approval prompts.");
    bypass.bypass = true;
    choices.append(bypass);

    if (catalog.available) {
        const bool editor = catalog.controls.size() > 1 || !catalog.unknownKeys.isEmpty();
        if (!editor) {
            const auto &control = catalog.controls.first();
            for (const auto &option : control.options) {
                QMap<QString, QString> selection;
                selection.insert(control.id, option.value);
                Choice choice;
                choice.label = option.label;
                choice.value = nativeValue(selection);
                choice.tooltip = option.description.isEmpty() ? control.description : option.description;
                if (choice.tooltip.isEmpty()) choice.tooltip = translated("Native option from %1.").arg(catalog.source);
                choice.bypass = isBypassText(option.value, option.label);
                choices.append(choice);
            }
        } else {
            Choice choice;
            choice.editor = true;
            choice.value = catalog.selected.isEmpty() ? QStringLiteral("__editor__") : nativeValue(catalog.selected);
            choice.label = catalog.selected.isEmpty() ? translated("Native permission settings…")
                : translated("Native permission settings… (%1)").arg(summary(catalog));
            choice.tooltip = translated("Choose each native permission option.");
            choice.unavailable = !catalog.selectedValid;
            choices.append(choice);
        }
    }

    if (catalog.nativeSaved) {
        if (catalog.malformed) {
            appendUnavailable(choices, catalog);
        } else if (catalog.selected.isEmpty()) {
            // An empty map is the provider default.
        } else if (!catalog.available) {
            appendUnavailable(choices, catalog);
        } else if (!catalog.selectedValid) {
            const bool editor = catalog.controls.size() > 1 || !catalog.unknownKeys.isEmpty();
            if (!editor) appendUnavailable(choices, catalog);
            // The editor choice above already selects unknown/withdrawn maps.
        }
    } else if (catalog.savedMode == QStringLiteral("auto")) {
        Choice choice;
        choice.label = translated("Auto (saved, unavailable)");
        choice.value = QStringLiteral("auto");
        choice.tooltip = translated("This saved legacy mode is honored for existing sessions but is not offered by the current harness.");
        choice.unavailable = true;
        choices.append(choice);
    } else if (catalog.savedMode != QStringLiteral("provider") && catalog.savedMode != QStringLiteral("bypass")) {
        appendUnavailable(choices, catalog);
    }
    return choices;
}

QList<Choice> sessionChoices(const Catalog &catalog, const QMap<QString, QString> &override)
{
    QList<Choice> choices;
    if (!catalog.present) {
        // Older CLIs accept only an explicit bypass per launch; the provider
        // default is already "Use Zerus account settings".
        Choice bypass;
        bypass.label = translated("Bypass permissions");
        bypass.value = QStringLiteral("bypass");
        bypass.tooltip = translated("Run tools without approval prompts.");
        bypass.bypass = true;
        choices.append(bypass);
        return choices;
    }
    Choice provider;
    provider.label = harnessSettingsLabel(catalog);
    provider.value = QStringLiteral("provider");
    provider.tooltip = translated("Use %1's own permission settings for this session, ignoring the Zerus account selection.").arg(catalog.harnessName);
    choices.append(provider);
    Choice bypass;
    bypass.label = translated("Bypass permissions");
    bypass.value = QStringLiteral("bypass");
    bypass.tooltip = translated("Run tools without approval prompts.");
    bypass.bypass = true;
    choices.append(bypass);
    if (!catalog.available) return choices;

    if (catalog.controls.size() == 1) {
        const auto &control = catalog.controls.first();
        for (const auto &option : control.options) {
            QMap<QString, QString> selection;
            selection.insert(control.id, option.value);
            Choice choice;
            choice.label = option.label;
            choice.value = nativeValue(selection);
            choice.tooltip = option.description.isEmpty() ? control.description : option.description;
            if (choice.tooltip.isEmpty()) choice.tooltip = translated("Native option from %1.").arg(catalog.source);
            choice.bypass = isBypassText(option.value, option.label);
            choices.append(choice);
        }
    } else {
        Choice choice;
        choice.editor = true;
        if (override.isEmpty()) {
            choice.value = QStringLiteral("__editor__");
            choice.label = translated("Native permission settings…");
        } else {
            choice.value = nativeValue(override);
            choice.label = translated("Native permission settings… (%1)").arg(summary(catalog, override));
            choice.unavailable = !selectionProblem(catalog, override).isEmpty();
        }
        choice.tooltip = translated("Choose each native permission option for this session.");
        choices.append(choice);
    }
    return choices;
}

QList<EditorRow> editorRows(const Catalog &catalog, const QMap<QString, QString> &selection)
{
    QList<EditorRow> rows;
    for (const auto &control : catalog.controls) {
        EditorRow row;
        row.id = control.id;
        row.label = control.label;
        row.description = control.description;
        row.options = control.options;
        row.selected = selection.value(control.id);
        if (!row.selected.isEmpty() && !findOption(control, row.selected)) row.unavailable = true;
        rows.append(row);
    }
    for (auto it = selection.begin(); it != selection.end(); ++it) {
        if (findControl(catalog, it.key())) continue;
        EditorRow row;
        row.id = it.key();
        row.label = translated("Unknown setting “%1”").arg(it.key());
        row.description = translated("This saved setting is not part of the current harness. Remove it before saving.");
        row.selected = it.value();
        row.unknown = true;
        rows.append(row);
    }
    return rows;
}

QString selectionProblem(const Catalog &catalog, const QMap<QString, QString> &selection)
{
    if (catalog.malformed) return translated("Saved native permission settings cannot be read. Choose \u201c%1\u201d to reset them.").arg(harnessSettingsLabel(catalog));
    if (nativeValue(selection).toUtf8().size() > 1024)
        return translated("Too many native settings are selected; remove some before saving.");
    for (auto it = selection.begin(); it != selection.end(); ++it) {
        if (!validControlId(it.key())) return translated("Unknown setting “%1” must be removed.").arg(it.key());
        const auto *control = findControl(catalog, it.key());
        if (!control) return translated("Unknown setting “%1” must be removed.").arg(it.key());
        if (!findOption(*control, it.value()))
            return translated("“%1” is no longer offered for %2.").arg(it.value()).arg(control->label);
    }
    for (const auto &control : catalog.controls) {
        if (!selection.contains(control.id)) continue;
        for (const auto &id : control.conflictsWith) {
            if (!selection.contains(id)) continue;
            const auto *other = findControl(catalog, id);
            return translated("%1 cannot be combined with %2. Choose “%3” for one of them.")
                .arg(control.label, other ? other->label : id, harnessSettingsLabel(catalog));
        }
    }
    return {};
}

QString sourceNote(const Catalog &catalog, const QString &machine)
{
    if (!catalog.present)
        return translated("This computer does not report native permission options. Harness settings and Bypass remain available.");
    if (!catalog.available) {
        QString note = translated("Native permissions could not be discovered on %1. Harness settings and Bypass remain available.").arg(machine);
        if (!catalog.statusReason.isEmpty()) note += QStringLiteral("\n") + catalog.statusReason;
        return note;
    }
    QString note = translated("Native options come from %1 on %2.")
        .arg(catalog.source.isEmpty() ? translated("this harness") : catalog.source).arg(machine);
    if (!catalog.sessionSupported)
        note += QStringLiteral("\n") + translated("This computer applies them to account defaults; a per-session choice is unavailable.");
    return note;
}

} // namespace PermissionCatalog
