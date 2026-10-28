#include <QtTest>
#include "WorktreeSettings.h"
// Also compile the ordinary Settings integration and its section routing.
#include "SettingsPage.h"
#include "FakeCli.h"
#include <QTemporaryDir>

class TestWorktreeSettings : public QObject {
    Q_OBJECT
private slots:
    void guardedSelectedComputerSaveAndUnsupportedPeer() {
        QTemporaryDir dir;QVERIFY(dir.isValid());const auto script=dir.filePath("hgs");
        const QByteArray source=R"PY(#!/usr/bin/env python3
import json,sys
from pathlib import Path
args=sys.argv[1:]
host=args.pop(0)[1:] if args and args[0].startswith('@') else ''
if '--help' in args:
 print('worktree-maintenance-v1' if host!='old' else 'legacy');sys.exit(0)
p=Path(__file__+'.'+(host or 'local'))
policy=json.loads(p.read_text()) if p.exists() else {'version':1,'revision':0,'enabled':True,'after_session':True,'unused_integrated':True,'missing':True,'grace_hours':24}
if args[2]=='set':
 incoming=json.load(sys.stdin)
 if incoming['revision']!=policy['revision']:sys.exit(1)
 policy=incoming;policy['revision']+=1;p.write_text(json.dumps(policy))
print(json.dumps({'schema':1,'policy':policy}))
)PY";
        QVERIFY(FakeCli::install(script,source));WorktreeSettings panel(FakeCli::program(script));panel.setPeers({"peer","old"});panel.show();
        auto *route=panel.findChild<QComboBox *>("worktreePolicyComputer");auto *enabled=panel.findChild<QCheckBox *>("worktreeCleanupEnabled");
        auto *apply=panel.findChild<QPushButton *>("applyWorktreePolicy");auto *grace=panel.findChild<QSpinBox *>("worktreeCleanupGrace");
        QVERIFY(route&&enabled&&apply&&grace);QVERIFY(!apply->isEnabled());
        route->setCurrentIndex(route->findData("peer"));QTRY_VERIFY(apply->isEnabled());QVERIFY(enabled->isChecked());QCOMPARE(grace->value(),24);
        enabled->setChecked(false);QVERIFY(!grace->isEnabled());apply->click();QTRY_VERIFY(apply->isEnabled());
        QFile stored(script+".peer");QVERIFY(stored.open(QIODevice::ReadOnly));const auto saved=QJsonDocument::fromJson(stored.readAll()).object();
        QVERIFY(!saved.value("enabled").toBool());QCOMPARE(saved.value("revision").toInt(),1);QVERIFY(!QFile::exists(script+".local"));
        route->setCurrentIndex(route->findData("old"));
        auto *status=panel.findChild<QLabel *>("worktreePolicyStatus");QTRY_VERIFY(status->text().contains("updated hgs"));QVERIFY(!apply->isEnabled());
        QVERIFY(!QFile::exists(script+".old"));
    }
};
QTEST_MAIN(TestWorktreeSettings)
#include "test_worktreesettings.moc"
