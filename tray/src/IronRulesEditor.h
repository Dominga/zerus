#pragma once
#include "IronRules.h"
#include <QPlainTextEdit>
#include <QInputDialog>
#include <QMessageBox>

// One line per addition. Validation refuses overflow rather than truncating
// the owner's rules; Rust applies the same bounds at the write boundary.
namespace IronRules {
inline QStringList lines(const QString &text) { return text.split('\n', Qt::SkipEmptyParts); }
inline QString problem(const QString &text)
{
    const auto value = lines(text);
    if (value.size() > MaxRules) return QObject::tr("Use at most 20 rules per level.");
    for (const auto &rule : value)
        if (rule.simplified().isEmpty() || rule.toUcs4().size() > MaxLength) return QObject::tr("Each rule must contain 1 to 300 characters.");
    return {};
}
inline QPlainTextEdit *editor(const QStringList &value, QWidget *parent = nullptr)
{
    auto *field = new QPlainTextEdit(value.join('\n'), parent);
    field->setObjectName("ironRulesEditor"); field->setMaximumHeight(110);
    field->setPlaceholderText(QObject::tr("One additional rule per line (20 maximum)"));
    return field;
}
inline bool edit(QWidget *parent, const QStringList &before, QStringList &after)
{
    bool ok = false;
    const auto text = QInputDialog::getMultiLineText(parent, QObject::tr("Member iron rules"),
        QObject::tr("Additions to project and team rules, one per line:"), before.join('\n'), &ok);
    if (!ok) return false;
    const auto error = problem(text);
    if (!error.isEmpty()) { QMessageBox::warning(parent, QObject::tr("Iron rules"), error); return false; }
    after = lines(text); return true;
}
}
