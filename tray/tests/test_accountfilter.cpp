#include <QtTest>
#include <QCheckBox>
#include <QMenu>
#include <QPushButton>
#include <QVBoxLayout>
#include "AccountFilter.h"

class TestAccountFilter : public QObject {
    Q_OBJECT
private slots:
    void recordedBindingsDistinguishAccountsAndMachines();
    void verifiedIdentitiesGroupWithoutChangingTheSelection();
    void missingBindingsNeverBecomeAllAccounts();
    void openMenuRetainsExactMembersAcrossCatalogUpdates();
    void duplicateCaptionsNameTheSpecificProfile();
    void missingGroupedBindingsKeepOneNamedChoice();
    void openMenuKeepsPresentationAndBindingsAcrossIdentitySplit();
private:
    static QJsonObject profile(const QString &host, const QString &id, const QString &identity, const QString &email) {
        return {{"host",host},{"machine",host.isEmpty()?QString("workstation"):host},{"id",id},{"provider","codex"},{"label","Work"},
            {"home","/fixture/"+id},{"usage",QJsonObject{{"status","ok"},{"identity",QJsonObject{{"account_id",identity},{"email",email}}}}}};
    }
    static SessionInfo session(const QString &id) { SessionInfo s; s.cmd="codex";s.accountId=id;return s; }
};
void TestAccountFilter::recordedBindingsDistinguishAccountsAndMachines() {
    SessionAccounts::Catalog catalog;
    catalog.reset({profile({},"one","identity-one","one@example.test"),profile({},"two","identity-two","two@example.test")});
    const auto one=session("one"), two=session("two");
    catalog.observe({},"workstation",one);catalog.observe({},"workstation",two);
    QCOMPARE(catalog.accounts().size(),2);
    const auto selected=QSet<QString>{SessionAccounts::binding({},one)};
    QVERIFY(SessionAccounts::accepts(selected,{},one));QVERIFY(!SessionAccounts::accepts(selected,{},two));
    QVERIFY(!SessionAccounts::accepts(selected,"mac",one));
    QVERIFY(catalog.description({},one).contains("one@example.test"));
    QVERIFY(catalog.description({},two).contains("two@example.test"));
    auto unknown=session({});catalog.observe({},"workstation",unknown);
    QVERIFY(catalog.description({},unknown).contains("Account not recorded"));
    QVERIFY(!catalog.description({},unknown).contains("one@example.test"));
    unknown.cmd="sh";catalog.observe({},"workstation",unknown);
    QVERIFY(catalog.description({},unknown).contains("No account (shell)"));
    auto legacy=session({});legacy.accountHome="/fixture/one";catalog.observe({},"workstation",legacy);
    QVERIFY(catalog.description({},legacy).contains("one@example.test"));
}
void TestAccountFilter::verifiedIdentitiesGroupWithoutChangingTheSelection() {
    SessionAccounts::Catalog catalog;
    catalog.reset({profile({},"one","shared","one@example.test"),profile("mac","other","shared","one@example.test")});
    QCOMPARE(catalog.accounts().size(),1);
    const auto account=catalog.accounts().first();QCOMPARE(account.bindings.size(),2);
    QVERIFY(account.bindings.contains(SessionAccounts::binding({},session("one"))));
    QVERIFY(account.bindings.contains(SessionAccounts::binding("mac",session("other"))));
    auto conflicting=profile("mac","other","different","one@example.test");
    catalog.reset({profile({},"one","shared","one@example.test"),conflicting});
    QCOMPARE(catalog.accounts().size(),2); // Equal labels and emails do not erase conflicting provider IDs.
}
void TestAccountFilter::missingBindingsNeverBecomeAllAccounts() {
    QWidget host;auto *layout=new QVBoxLayout(&host);auto *filter=new AccountFilter;
    layout->addWidget(filter->button());layout->addWidget(filter);host.show();
    SessionAccounts::Catalog catalog;catalog.reset({profile({},"one","shared","one@example.test")});
    filter->setAccounts(catalog.accounts());const auto selected=catalog.accounts().first().bindings;
    filter->setSelection(selected);filter->setAccounts({});
    QCOMPARE(filter->selection(),selected);QCOMPARE(filter->findChildren<QPushButton *>("accountFilterChip").size(),1);
    auto *menu=filter->button()->menu();menu->popup(host.mapToGlobal(QPoint(10,10)));QTRY_VERIFY(menu->isVisible());
    const auto choices=menu->findChildren<QCheckBox *>("accountFilterChoice");QCOMPARE(choices.size(),1);QVERIFY(choices.first()->isChecked());
    QVERIFY(choices.first()->text().contains("one@example.test"));
    menu->findChild<QPushButton *>("allAccountFilters")->click();QVERIFY(filter->selection().isEmpty());
}
void TestAccountFilter::openMenuRetainsExactMembersAcrossCatalogUpdates() {
    QWidget host;auto *layout=new QVBoxLayout(&host);auto *filter=new AccountFilter;
    layout->addWidget(filter->button());layout->addWidget(filter);host.show();
    SessionAccounts::Catalog catalog;catalog.reset({profile({},"one","shared","one@example.test")});
    filter->setAccounts(catalog.accounts());const auto first=catalog.accounts().first().bindings;
    filter->setSessionCounts({{*first.cbegin(),3}},"All");
    auto *menu=filter->button()->menu();menu->popup(host.mapToGlobal(QPoint(10,10)));QTRY_VERIFY(menu->isVisible());
    auto *choice=menu->findChild<QCheckBox *>("accountFilterChoice");QVERIFY(choice);QCOMPARE(choice->property("sessionCount").toInt(),3);
    catalog.reset({profile({},"one","shared","one@example.test"),profile("mac","other","shared","one@example.test")});
    filter->setAccounts(catalog.accounts());QCOMPARE(menu->findChild<QCheckBox *>("accountFilterChoice"),choice);
    choice->click();QCOMPARE(filter->selection(),first);QVERIFY(menu->isVisible()); // No silent new-machine expansion.
    menu->hide();menu->popup(host.mapToGlobal(QPoint(10,10)));QTRY_VERIFY(menu->isVisible());
    choice=menu->findChild<QCheckBox *>("accountFilterChoice");QCOMPARE(choice->checkState(),Qt::PartiallyChecked);
    choice->click();QCOMPARE(filter->selection(),catalog.accounts().first().bindings);
    filter->findChild<QPushButton *>("accountFilterChip")->click();QVERIFY(filter->selection().isEmpty());
}
void TestAccountFilter::duplicateCaptionsNameTheSpecificProfile() {
    QWidget host;auto *layout=new QVBoxLayout(&host);auto *filter=new AccountFilter;
    layout->addWidget(filter->button());layout->addWidget(filter);host.show();
    SessionAccounts::Catalog catalog;
    catalog.reset({profile({},"one","identity-one","shared@example.test"),profile({},"two","identity-two","shared@example.test")});
    filter->setAccounts(catalog.accounts());
    auto *menu=filter->button()->menu();menu->popup(host.mapToGlobal(QPoint(10,10)));QTRY_VERIFY(menu->isVisible());
    const auto choices=menu->findChildren<QCheckBox *>("accountFilterChoice");QCOMPARE(choices.size(),2);
    QVERIFY(choices[0]->text()!=choices[1]->text());QVERIFY(choices[0]->accessibleName()!=choices[1]->accessibleName());
    for(auto *choice:choices) {
        const auto members=choice->property("bindings").toStringList();QCOMPARE(members.size(),1);
        const auto id=QJsonDocument::fromJson(members.first().toUtf8()).array()[3].toString();
        QVERIFY(choice->text().contains(id));choice->click();QVERIFY(filter->selection().contains(members.first()));
    }
}
void TestAccountFilter::missingGroupedBindingsKeepOneNamedChoice() {
    QWidget host;auto *layout=new QVBoxLayout(&host);auto *filter=new AccountFilter;
    layout->addWidget(filter->button());layout->addWidget(filter);host.show();
    SessionAccounts::Catalog catalog;
    catalog.reset({profile({},"one","shared","shared@example.test"),profile("mac","other","shared","shared@example.test")});
    filter->setAccounts(catalog.accounts());const auto selected=catalog.accounts().first().bindings;
    filter->setSelection(selected);filter->setAccounts({});
    QCOMPARE(filter->selection(),selected);QCOMPARE(filter->findChildren<QPushButton *>("accountFilterChip").size(),1);
    auto *menu=filter->button()->menu();menu->popup(host.mapToGlobal(QPoint(10,10)));QTRY_VERIFY(menu->isVisible());
    const auto choices=menu->findChildren<QCheckBox *>("accountFilterChoice");QCOMPARE(choices.size(),1);
    QVERIFY(choices.first()->text().contains("shared@example.test"));choices.first()->click();QVERIFY(filter->selection().isEmpty());
}
void TestAccountFilter::openMenuKeepsPresentationAndBindingsAcrossIdentitySplit() {
    QWidget host;auto *layout=new QVBoxLayout(&host);auto *filter=new AccountFilter;
    layout->addWidget(filter->button());layout->addWidget(filter);host.show();
    SessionAccounts::Catalog catalog;
    catalog.reset({profile({},"one","shared","shared@example.test"),profile("mac","other","shared","shared@example.test")});
    filter->setAccounts(catalog.accounts());const auto members=catalog.accounts().first().bindings;
    auto *menu=filter->button()->menu();menu->popup(host.mapToGlobal(QPoint(10,10)));QTRY_VERIFY(menu->isVisible());
    auto *choice=menu->findChild<QCheckBox *>("accountFilterChoice");QVERIFY(choice);
    const auto text=choice->text(), tooltip=choice->toolTip(), accessible=choice->accessibleName();
    const auto snapshot=choice->property("bindings").toStringList();QCOMPARE(snapshot.size(),2);
    catalog.reset({profile({},"one","identity-one","one@example.test"),profile("mac","other","identity-two","two@example.test")});
    QCOMPARE(catalog.accounts().size(),2);filter->setAccounts(catalog.accounts());
    QCOMPARE(menu->findChild<QCheckBox *>("accountFilterChoice"),choice);
    QCOMPARE(choice->text(),text);QCOMPARE(choice->toolTip(),tooltip);QCOMPARE(choice->accessibleName(),accessible);
    QCOMPARE(choice->property("bindings").toStringList(),snapshot);
    choice->click();QCOMPARE(filter->selection(),members);QVERIFY(menu->isVisible());
    menu->hide();menu->popup(host.mapToGlobal(QPoint(10,10)));QTRY_VERIFY(menu->isVisible());
    const auto choices=menu->findChildren<QCheckBox *>("accountFilterChoice");QCOMPARE(choices.size(),2);
    for(auto *current:choices) {
        const auto binding=current->property("bindings").toStringList();QCOMPARE(binding.size(),1);
        const auto profileId=QJsonDocument::fromJson(binding.first().toUtf8()).array()[3].toString();
        QVERIFY(current->toolTip().contains(profileId=="one"?"one@example.test":"two@example.test"));
        QVERIFY(current->isChecked());
    }
}
QTEST_MAIN(TestAccountFilter)
#include "test_accountfilter.moc"
