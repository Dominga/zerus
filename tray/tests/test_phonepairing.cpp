#include <QtTest>
#include <QFile>
#include <QClipboard>
#include <QLabel>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QScreen>
#include <QScrollArea>
#include <QTimer>
#include <QTemporaryDir>
#include <QTreeWidget>
#include <QToolButton>
#include "FakeCli.h"
#include "PhonePairing.h"

namespace {
const char *Device = "00000000-0000-4000-8000-0000000000d1";
}

class TestPhonePairing : public QObject {
    Q_OBJECT
    QTemporaryDir m_dir;
    QString m_hgs;
    void touch(const QString &name) { QFile file(m_dir.filePath(name)); QVERIFY(file.open(QIODevice::WriteOnly)); }
    QStringList calls() const {
        QFile file(m_dir.filePath("calls")); if (!file.open(QIODevice::ReadOnly)) return {};
        return QString::fromUtf8(file.readAll()).split('\n', Qt::SkipEmptyParts);
    }
    static QStringList children(const QTreeWidget *tree, int column) {
        QStringList out;
        for (int i = 0; i < tree->topLevelItemCount(); ++i)
            for (int j = 0; j < tree->topLevelItem(i)->childCount(); ++j) out << tree->topLevelItem(i)->child(j)->text(column);
        return out;
    }
private slots:
    void init() {
        QVERIFY(m_dir.isValid());
        for (const auto *name : {"calls", "revoked", "old-peer", "no-connection", "other-workspace", "fail-revoke", "long-code", "too-large-code", "fail-invite", "workspace-switched", "hold-phone-list", "phone-list-held", "release-phone-list"}) QFile::remove(m_dir.filePath(name));
        m_hgs = m_dir.filePath("hgs");
        QVERIFY(FakeCli::install(m_hgs, R"PY(#!/usr/bin/env python3
import json, pathlib, sys, time
root = pathlib.Path(__file__).parent
args = sys.argv[1:]
with (root / 'calls').open('a') as file: file.write(' '.join(args) + '\n')
host = args.pop(0)[1:] if args and args[0].startswith('@') else ''
if args == ['--help']:
    print('usage: hgs [@host] <cmd>')
    if not (root / ('old-' + host)).exists():
        print('       hgs [@host] mobile phones|invite|revoke-phone <id> --json   phones')
elif args == ['machine', 'ls']:
    print(json.dumps({'local': 'desk', 'machines': [{'alias': 'peer', 'enabled': True}, {'alias': 'off', 'enabled': False}]}))
elif args == ['mobile', 'phones', '--json']:
    switched = (root / 'workspace-switched').exists()
    if host == 'peer' and (root / 'hold-phone-list').exists():
        (root / 'phone-list-held').touch()
        deadline = time.monotonic() + 3
        while not (root / 'release-phone-list').exists() and time.monotonic() < deadline: time.sleep(.01)
    if host != 'peer' or (root / 'no-connection').exists():
        print(json.dumps({'configured': False}))
    else:
        devices = [{'id': '00000000-0000-4000-8000-0000000000d1', 'name': 'Zerus iOS', 'platform': 'ios', 'paired_at': 1700000000,
                    'last_seen_at': 1700000300, 'added_by': {'computer_id': 'c', 'name': 'Gateway'}},
                   {'id': '00000000-0000-4000-8000-0000000000d2', 'name': 'Zerus Android', 'platform': 'android', 'paired_at': None,
                    'last_seen_at': None, 'added_by': None}]
        if (root / 'revoked').exists(): devices = devices[1:]
        print(json.dumps({'configured': True, 'server_url': 'https://relay.example.test', 'workspace_id': 'w2' if switched else 'w1',
                          'workspace_name': 'Example workspace', 'devices': devices}))
elif args[:2] == ['mobile', 'invite'] and args[2:] in (['--json'], ['--workspace', 'w1', '--server', 'https://relay.example.test', '--json'], ['--workspace', 'w2', '--server', 'https://relay.example.test', '--json']):
    if (root / 'fail-invite').exists(): print('hgs: The relay is temporarily unavailable', file=sys.stderr); sys.exit(1)
    code = 'ze1.' + ('synthetic-invitation-' * 90) if (root / 'long-code').exists() else 'synthetic_pair-code_0123456789'
    if (root / 'too-large-code').exists(): code = 'ze1.' + ('synthetic-invitation-' * 160)
    print(json.dumps({'workspace_id': 'w2' if (root / 'other-workspace').exists() or (root / 'workspace-switched').exists() else 'w1', 'workspace_name': 'Example workspace', 'pair_code': code,
                      'encrypted': (root / 'long-code').exists() or (root / 'workspace-switched').exists() or (root / 'too-large-code').exists(),
                      'expires_at': float((root / 'expires').read_text()), 'server_url': 'https://relay.example.test',
                      'pair_url': 'zerus://pair?server=https%3A%2F%2Frelay.example.test&code=' + code}))
elif args[:2] == ['mobile', 'revoke-phone'] and args[3:] in (['--json'], ['--workspace', 'w1', '--server', 'https://relay.example.test', '--json']):
    if (root / 'fail-revoke').exists(): print('hgs: The relay is temporarily unavailable', file=sys.stderr); sys.exit(1)
    (root / 'revoked').write_text(args[2]); print(json.dumps({'revoked': True, 'device_id': args[2]}))
else:
    print('unexpected', args, file=sys.stderr); sys.exit(2)
)PY"));
    }
    void encodesPairingLinksAsQrCodes() {
        int size = 0;
        const auto modules = PhonePairing::encode("zerus://pair?server=https%3A%2F%2Frelay.zerus.dev&code=" + QString(43, 'A'), &size);
        QVERIFY(size >= 21); QCOMPARE((size - 17) % 4, 0); QCOMPARE(int(modules.size()), size * size);
        // The top-left finder pattern: a dark 7-module edge, then the light separator.
        for (int x = 0; x < 7; ++x) QVERIFY(modules[size_t(x)]);
        QVERIFY(!modules[7]);
        QVERIFY(PhonePairing::encode({}, &size).empty()); QCOMPARE(size, 0);
    }
    void encryptedInvitationFitsAReadableDialog_data() {
        QTest::addColumn<bool>("dark"); QTest::addColumn<int>("fontSize");
        QTest::newRow("light") << false << 13;
        QTest::newRow("dark") << true << 13;
        QTest::newRow("large-light") << false << 20;
        QTest::newRow("large-dark") << true << 20;
    }
    void encryptedInvitationFitsAReadableDialog() {
        QFETCH(bool, dark); QFETCH(int, fontSize);
        touch("long-code");
        QFile expires(m_dir.filePath("expires")); QVERIFY(expires.open(QIODevice::WriteOnly)); expires.write(QByteArray::number(QDateTime::currentSecsSinceEpoch() + 600)); expires.close();
        PhonePairing::Dialog dialog(FakeCli::program(m_hgs), {{"peer", "w1", "Example workspace", "https://relay.example.test"}}, 0);
        dialog.setStyleSheet(QString("QWidget { background:%1; color:%2; font-size:%3px; }").arg(dark ? "#202426" : "#f5f7fa", dark ? "#e8edf4" : "#202426").arg(fontSize));
        dialog.show();
        auto *copy = dialog.findChild<QPushButton *>("copyPairingCode");
        QTRY_VERIFY(copy->isEnabled());
        const auto available = dialog.screen()->availableGeometry();
        QVERIFY2(dialog.width() <= qMin(760, available.width()), qPrintable(QString::number(dialog.width())));
        auto *code = dynamic_cast<PhonePairing::QrCodeView *>(dialog.findChild<QWidget *>("phonePairingCode")); QVERIFY(code);
        QVERIFY(code->modules() > 100);
        QTRY_VERIFY(qMin(code->width(), code->height()) >= (code->modules() + 8) * (fontSize == 13 ? 3 : 2));
        auto *viewport = dialog.findChild<QScrollArea *>("phonePairingScroll")->viewport();
        QTRY_VERIFY(viewport->rect().contains(QRect(code->mapTo(viewport, QPoint()), code->size())));
        auto *close = dialog.findChild<QPushButton *>("closePhonePairing");
        QVERIFY(dialog.rect().contains(QRect(close->mapTo(&dialog, QPoint()), close->size())));
        auto *manual = dialog.findChild<QToolButton *>("showPairingCode"); QVERIFY(manual);
        auto *text = dialog.findChild<QPlainTextEdit *>("phonePairingCodeText"); QVERIFY(text);
        QVERIFY(!text->isVisible()); manual->click(); QTRY_VERIFY(text->isVisible());
        const QString expected = "ze1." + QString("synthetic-invitation-").repeated(90);
        QCOMPARE(text->toPlainText(), expected);
        copy->click(); QCOMPARE(QApplication::clipboard()->text(), expected);
        QCOMPARE(text->horizontalScrollBarPolicy(), Qt::ScrollBarAlwaysOff);
        QVERIFY(text->height() < 160);
        QVERIFY(dialog.width() <= qMin(760, available.width()));
        manual->click(); QTRY_VERIFY(!text->isVisible());
        const auto preview = qEnvironmentVariable("HGS_PHONE_PAIRING_PREVIEW");
        if (!preview.isEmpty()) {
            QDir().mkpath(preview); QTest::qWait(30);
            QVERIFY(dialog.grab().save(preview + QString("/pairing-%1.png").arg(QLatin1String(QTest::currentDataTag()))));
        }
        dialog.resize(500, 480); QTRY_VERIFY(dialog.rect().contains(QRect(close->mapTo(&dialog, QPoint()), close->size())));
        close->click(); QVERIFY(text->toPlainText().isEmpty());
    }
    void unavailableOrOversizeQrCodesShowAnExplanation_data() {
        QTest::addColumn<bool>("failed");
        QTest::newRow("relay-failure") << true;
        QTest::newRow("qr-capacity") << false;
    }
    void unavailableOrOversizeQrCodesShowAnExplanation() {
        QFETCH(bool, failed); touch(failed ? "fail-invite" : "too-large-code");
        QFile expires(m_dir.filePath("expires")); QVERIFY(expires.open(QIODevice::WriteOnly)); expires.write(QByteArray::number(QDateTime::currentSecsSinceEpoch() + 600)); expires.close();
        PhonePairing::Dialog dialog(FakeCli::program(m_hgs), {{"peer", "w1", "Example workspace", "https://relay.example.test"}}, 0);
        dialog.show(); auto *placeholder = dialog.findChild<QLabel *>("phonePairingQrStatus"); QVERIFY(placeholder);
        QVERIFY(placeholder->text().contains("Requesting"));
        QTRY_VERIFY(dialog.findChild<QPushButton *>("newPairingCode")->isEnabled());
        QVERIFY(placeholder->isVisible());
        QVERIFY(placeholder->text().contains(failed ? "unavailable" : "too large"));
        QCOMPARE(dialog.findChild<QPushButton *>("copyPairingCode")->isEnabled(), !failed);
        const auto log = calls(); int invitations = 0; for (const auto &call : log) invitations += call.contains("mobile invite");
        QCOMPARE(invitations, 1);
    }
    void pairingRefreshesTheWorkspaceBeforeIssuingAnInvitation_data() {
        QTest::addColumn<bool>("changeWhileReading");
        QTest::newRow("changed-before-pair") << false;
        QTest::newRow("changed-during-refresh") << true;
    }
    void pairingRefreshesTheWorkspaceBeforeIssuingAnInvitation() {
        QFETCH(bool, changeWhileReading);
        QFile expires(m_dir.filePath("expires")); QVERIFY(expires.open(QIODevice::WriteOnly)); expires.write(QByteArray::number(QDateTime::currentSecsSinceEpoch() + 600)); expires.close();
        PhonePairing::Panel panel(FakeCli::program(m_hgs)); panel.refresh();
        QTRY_COMPARE(panel.targets().size(), 1); QCOMPARE(panel.targets().first().workspaceId, QString("w1"));
        if (changeWhileReading) touch("hold-phone-list"); else touch("workspace-switched");
        bool closed = false; QString shown;
        QTimer closer;
        connect(&closer, &QTimer::timeout, &panel, [&] {
            for (auto *widget : QApplication::topLevelWidgets()) {
                auto *dialog = dynamic_cast<PhonePairing::Dialog *>(widget);
                if (!dialog || !dialog->isVisible()) continue;
                const auto status = dialog->findChild<QLabel *>("phonePairingStatus")->text();
                if (status.startsWith("Requesting")) continue;
                shown = status; closed = true; dialog->reject();
            }
        });
        closer.start(25);
        QTimer::singleShot(0, &panel, [&] { panel.findChild<QPushButton *>("pairPhone")->click(); });
        if (changeWhileReading) {
            QTRY_VERIFY(QFileInfo::exists(m_dir.filePath("phone-list-held")));
            touch("workspace-switched"); panel.refresh(); touch("release-phone-list");
        }
        QTRY_VERIFY(closed);
        QVERIFY2(!shown.contains("another workspace"), qPrintable(shown));
        QVERIFY(calls().contains("@peer mobile invite --workspace w2 --server https://relay.example.test --json"));
        QCOMPARE(calls().count("@peer mobile invite --workspace w1 --server https://relay.example.test --json"), 0);
        QCOMPARE(calls().count("@peer mobile invite --workspace w2 --server https://relay.example.test --json"), 1);
    }
    void listsPhonesOfReachableWorkspacesAndSkipsDisabledComputers() {
        PhonePairing::Panel panel(FakeCli::program(m_hgs));
        auto *tree = panel.findChild<QTreeWidget *>("linkedPhones"); QVERIFY(tree);
        panel.refresh();
        QTRY_COMPARE(tree->topLevelItemCount(), 1);
        QCOMPARE(tree->topLevelItem(0)->text(0), QString("Example workspace · via peer"));
        QCOMPARE(children(tree, 0), (QStringList{"Zerus iOS", "Zerus Android"}));
        QCOMPARE(children(tree, 1), (QStringList{"iOS", "Android"}));
        QCOMPARE(children(tree, 4), (QStringList{"Gateway", "—"}));
        QCOMPARE(children(tree, 2).value(1), QString("—"));
        QVERIFY(panel.findChild<QPushButton *>("pairPhone")->isEnabled());
        QCOMPARE(panel.findChild<QLabel *>("phonesStatus")->text(), QString("2 phones"));
        const auto log = calls();
        QVERIFY(log.contains("mobile phones --json")); QVERIFY(log.contains("@peer mobile phones --json"));
        for (const auto &call : log) QVERIFY2(!call.startsWith("@off"), qPrintable(call));
        QCOMPARE(panel.targets().size(), 1); QCOMPARE(panel.targets().first().host, QString("peer"));
    }
    void revokesThroughTheSameRouteOnlyAfterConfirmation() {
        bool accept = false; QString asked;
        PhonePairing::Panel panel(FakeCli::program(m_hgs), nullptr, [&](const QString &phone, const QString &workspace) {
            asked = phone + " / " + workspace; return accept;
        });
        auto *tree = panel.findChild<QTreeWidget *>("linkedPhones"); auto *revoke = panel.findChild<QPushButton *>("revokePhone");
        panel.refresh();
        QTRY_COMPARE(children(tree, 0).size(), 2);
        QVERIFY(!revoke->isEnabled());
        tree->setCurrentItem(tree->topLevelItem(0)->child(0));
        QVERIFY(revoke->isEnabled());
        revoke->click();
        QCOMPARE(asked, QString("Zerus iOS / Example workspace"));
        QTest::qWait(200);
        for (const auto &call : calls()) QVERIFY(!call.contains("revoke-phone"));
        accept = true; revoke->click();
        QTRY_COMPARE(children(tree, 0), QStringList{"Zerus Android"});
        QVERIFY(calls().contains(QString("@peer mobile revoke-phone %1 --workspace w1 --server https://relay.example.test --json").arg(Device)));
    }
    void aFailedRevocationStaysVisibleAndIsNotRetried() {
        touch("fail-revoke");
        PhonePairing::Panel panel(FakeCli::program(m_hgs), nullptr, [](const QString &, const QString &) { return true; });
        auto *tree = panel.findChild<QTreeWidget *>("linkedPhones"); auto *status = panel.findChild<QLabel *>("phonesStatus");
        panel.refresh();
        QTRY_COMPARE(children(tree, 0).size(), 2);
        tree->setCurrentItem(tree->topLevelItem(0)->child(0));
        panel.findChild<QPushButton *>("revokePhone")->click();
        QTRY_VERIFY(status->text().contains("Could not confirm revoking Zerus iOS"));
        QVERIFY(status->text().contains("The relay is temporarily unavailable"));
        QTest::qWait(500);
        QVERIFY(status->text().contains("Could not confirm revoking Zerus iOS"));
        QCOMPARE(children(tree, 0).size(), 2);
        int revokes = 0; for (const auto &call : calls()) revokes += call.contains("revoke-phone");
        QCOMPARE(revokes, 1);
        // An explicit refresh shows the current list and clears the notice.
        panel.findChild<QPushButton *>("refreshPhones")->click();
        QTRY_COMPARE(status->text(), QString("2 phones"));
    }
    void thePairingDialogNeverShowsAnotherWorkspacesCode() {
        touch("other-workspace");
        QFile expires(m_dir.filePath("expires")); QVERIFY(expires.open(QIODevice::WriteOnly)); expires.write("4000000000"); expires.close();
        PhonePairing::Dialog dialog(FakeCli::program(m_hgs), {{"peer", "w1", "Example workspace", "https://relay.example.test"}}, 0);
        auto *status = dialog.findChild<QLabel *>("phonePairingStatus");
        QTRY_VERIFY(status->text().contains("another workspace"));
        auto *code = dynamic_cast<PhonePairing::QrCodeView *>(dialog.findChild<QWidget *>("phonePairingCode"));
        QVERIFY(code->text().isEmpty());
        QVERIFY(!dialog.findChild<QPushButton *>("copyPairingCode")->isEnabled());
        QVERIFY(dialog.findChild<QPlainTextEdit *>("phonePairingCodeText")->toPlainText() != QString("synthetic_pair-code_0123456789"));
    }
    void anOlderComputerIsNeverAskedToRunMobile() {
        touch("old-peer");
        PhonePairing::Panel panel(FakeCli::program(m_hgs));
        auto *status = panel.findChild<QLabel *>("phonesStatus");
        panel.refresh();
        QTRY_VERIFY(status->text().contains("Update Zerus on peer to manage phones"));
        for (const auto &call : calls()) QVERIFY2(!call.startsWith("@peer mobile"), qPrintable(call));
        QVERIFY(status->text().contains("Settings → Mobile connection"));
        QVERIFY(!panel.findChild<QPushButton *>("pairPhone")->isEnabled());
    }
    void thePairingDialogShowsASingleUseCodeUntilItExpires() {
        double now = 1'000'000;
        QFile expires(m_dir.filePath("expires")); QVERIFY(expires.open(QIODevice::WriteOnly)); expires.write("1000600"); expires.close();
        PhonePairing::Dialog dialog(FakeCli::program(m_hgs), {{"peer", "w1", "Example workspace", "https://relay.example.test"}}, 0,
                                    nullptr, [&now] { return now; });
        auto *code = dynamic_cast<PhonePairing::QrCodeView *>(dialog.findChild<QWidget *>("phonePairingCode")); QVERIFY(code);
        QTRY_COMPARE(code->text(), QString("zerus://pair?server=https%3A%2F%2Frelay.example.test&code=synthetic_pair-code_0123456789"));
        QVERIFY(code->modules() >= 21);
        QCOMPARE(dialog.findChild<QPlainTextEdit *>("phonePairingCodeText")->toPlainText(), QString("synthetic_pair-code_0123456789"));
        QCOMPARE(dialog.findChild<QLabel *>("phonePairingServer")->text(), QString("https://relay.example.test"));
        QCOMPARE(dialog.findChild<QLabel *>("phonePairingExpiry")->text(), QString("Expires in 10:00"));
        QVERIFY(calls().contains("@peer mobile invite --workspace w1 --server https://relay.example.test --json"));
        auto *copy = dialog.findChild<QPushButton *>("copyPairingCode"); QVERIFY(copy->isEnabled());
        now = 1'000'601;
        QTRY_VERIFY_WITH_TIMEOUT(dialog.findChild<QLabel *>("phonePairingExpiry")->text().contains("expired"), 3000);
        QVERIFY(!copy->isEnabled());
        dialog.findChild<QPushButton *>("closePhonePairing")->click();
        QVERIFY(code->text().isEmpty());
    }
};

QTEST_MAIN(TestPhonePairing)
#include "test_phonepairing.moc"
