#pragma once
#include <QJsonObject>
#include <QList>
#include <QMap>
#include <QString>
#include <QStringList>

// Native permission discovery, shared by Accounts and New session. A computer's
// account row describes its harness in permission_catalog: opaque control ids,
// human labels and the values its own CLI accepts. Zerus stores a selection as
// "native:<compact sorted JSON object>" and never turns a value into argv here.
namespace PermissionCatalog {

struct Option {
    QString value;
    QString label;
    QString description;
};

struct Control {
    QString id;
    QString label;
    QString description;
    QList<Option> options;
    QStringList conflictsWith;
};

// One row of a selector. `value` is what the caller persists: a legacy
// provider/bypass/auto selector, a native: map, or the editor sentinel.
struct Choice {
    QString label;
    QString value;
    QString tooltip;
    bool bypass = false;
    bool unavailable = false;
    bool editor = false;
};

// One control in the native settings editor. Unknown keys keep their saved
// value visible until the user explicitly removes them.
struct EditorRow {
    QString id;
    QString label;
    QString description;
    QString selected;
    bool unknown = false;
    bool malformed = false;
    bool unavailable = false;
    QList<Option> options;
};

struct Catalog {
    QString harnessName;
    bool present = false;             // permission_catalog was returned at all
    bool available = false;           // status available and at least one control
    bool sessionCapabilityReported = false;
    bool sessionSupported = false;    // permission_catalog.session_supported
    QString status;
    QString statusReason;
    QString source;
    QString version;
    QList<Control> controls;
    QMap<QString, QString> selected;  // control id -> saved value
    QString savedMode = QStringLiteral("provider");
    bool nativeSaved = false;
    bool malformed = false;           // saved native: value is not a string map
    bool selectedValid = true;        // no unknown ids or withdrawn values
    QStringList unknownKeys;
    QStringList unavailableControls;
    QString currentSource;            // permission_current.source
    QString currentDetail;            // permission_current.detail
    QString effectiveMode;            // legacy effective_permission_mode
    QString legacyDetail;             // legacy permission_detail
    QMap<QString, QString> currentValues;
};

Catalog parse(const QJsonObject &profile);
bool validControlId(const QString &id);
bool decodeNative(const QString &mode, QMap<QString, QString> *values);
QString nativeValue(const QMap<QString, QString> &values);
QString optionLabel(const Catalog &catalog, const QString &control, const QString &value);
QString summary(const Catalog &catalog);
QString summary(const Catalog &catalog, const QMap<QString, QString> &values);
bool isBypass(const Catalog &catalog);
QString displayLabel(const QString &legacyMode);
QString harnessSettingsLabel(const Catalog &catalog);
QList<Choice> accountChoices(const Catalog &catalog);
// Choices after "Use Zerus account settings"; empty when the catalog does not launch with
// permissions. `override` is this session's native map, if one was chosen.
QList<Choice> sessionChoices(const Catalog &catalog, const QMap<QString, QString> &override);
QList<EditorRow> editorRows(const Catalog &catalog, const QMap<QString, QString> &selection);
QString selectionProblem(const Catalog &catalog, const QMap<QString, QString> &selection);
QString sourceNote(const Catalog &catalog, const QString &machine);

} // namespace PermissionCatalog
