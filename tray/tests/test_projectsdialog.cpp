#include <QtTest>
#include "Settings.h"
#include <QApplication>
#include "JsonSettings.h"
#include "ProjectsDialog.h"
#include "IronRules.h"
#include "CloneRepositoryDialog.h"
#include "PopupMouse.h"
#include "FakeCli.h"
#include "CollapsibleBlock.h"
#include "PageShots.h"
#include <QInputDialog>
#include <QColorDialog>
#include <QCheckBox>
#include <QJsonDocument>
#include <QJsonArray>
#include <QLabel>
#include <QListWidget>
#include <QMenu>
#include <QSettings>
#include <QTemporaryDir>
#include <QHeaderView>
#include <QPushButton>
#include <QSplitter>
#include <QTableWidget>
#include <QTreeWidget>
#include <QScrollArea>
#include <QScrollBar>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFile>
#include <QLineEdit>
#include <functional>
#include <QTimer>
#include <QPointer>
#include <QMessageBox>
#include <QElapsedTimer>

class TestProjectsDialog : public QObject {
    Q_OBJECT
private slots:
    void projectConnectionsGuideExplainsMissingCatalogsWithoutJoining();
    void cloneAddsFolderWithoutLaunchingSession();
    void projectPageHasHeaderAndCollapsibleBlocks();
    void projectPageMatchesThePrototype();
    void pageMessagesStayInViewAndOnlyRefusalsAreRed();
    void renameKeepsItsProjectOrIsCancelled();
    void narrowWindowKeepsTheKindColumnReachable();
    void folderDialogsHaveShots();
    void folderKindsFollowTheFleetAndTheTeams();
    void projectSessionsFollowTheFleetWhileShown();
    void projectSessionRowsAreBounded();
    void worktreeOverviewIsVisibleWithoutSelectingAProjectFolder();
    void projectColorChangesRequireApply_data();
    void projectColorChangesRequireApply();
    void customProjectColorRemainsStaged_data();
    void customProjectColorRemainsStaged();
    void emptyListContextCreatesProject();
    void sortingKeepsFolderIdentityAndLaunchTarget();
    void folderTransfersKeepMenuTargetsAndPersist_data();
    void folderTransfersKeepMenuTargetsAndPersist();
    void folderTransferMenusRejectStaleChanges_data();
    void folderTransferMenusRejectStaleChanges();
    void folderTransferMenusRespectVisibleProjects();
    void folderSplitPrioritizesListAndRemembersAdjustment();
    void folderDialogTakesDriveFoldersOnlyOnLocalWindows_data();
    void folderDialogTakesDriveFoldersOnlyOnLocalWindows();
    void legacyAliasesOfALocalWindowsComputerBecomeFolders();
    void legacyImportReportsAliasesItCannotAdd();
    void acceptsPlainName();
    void rejectsSlashAndSpace();
    void rejectsEmpty();
    void warnsOnDotOrColon();
    void nameDefaultsToBasename();
};

void TestProjectsDialog::projectConnectionsGuideExplainsMissingCatalogsWithoutJoining()
{
    HgsClient client("/nonexistent/hgs"); SessionOrganization projects;
    projects.markImported("desktop"); projects.markImported("laptop"); projects.markImported("office");
    ProjectsDialog dialog(&client,nullptr,false,&projects);
    FleetState fleet; BoxState local; local.host="desktop"; local.ok=true; fleet.setLocal(local,1);
    for(const auto &alias:{"laptop","office"}){BoxState peer;peer.host=alias;peer.ok=true;fleet.setPeer(peer,1);}
    dialog.setFleet(fleet);dialog.show();
    auto *guide=dialog.findChild<QWidget *>("projectConnectionGuide");
    auto *hint=dialog.findChild<QLabel *>("projectConnectionHint");
    auto *connect=dialog.findChild<QPushButton *>("connectProjectCatalogs");
    QVERIFY(guide);QVERIFY(connect);QVERIFY(guide->isHidden()); // unknown is not unjoined proof
    QSignalSpy requested(&dialog,&ProjectsDialog::connectSwarmRequested);
    QSignalSpy changed(&dialog,&ProjectsDialog::organizationChanged);
    dialog.setSwarmSnapshot({{"node_id","desktop-id"},{"peers",QJsonObject{{"office",QJsonObject{{"node","office-id"},{"error","offline"}}}}}});
    QVERIFY(!guide->isHidden());QVERIFY(hint->text().contains("laptop"));QVERIFY(!hint->text().contains("office"));
    QVERIFY(hint->text().contains("Preview"));QVERIFY(hint->text().contains("explicitly join"));
    connect->click();QCOMPARE(requested.size(),1);QCOMPARE(requested[0][0].toString(),QString("laptop"));
    QCOMPARE(changed.size(),0); // navigation is not enrollment or membership mutation
    dialog.setSwarmSnapshot({{"node_id","desktop-id"},{"peers",QJsonObject{{"laptop",QJsonObject{}},{"office",QJsonObject{}}}}});
    QVERIFY(guide->isHidden());QCOMPARE(changed.size(),0);
}

// U25 part 7 (spec 7): the project header with its counts and actions, and
// Folders, Sessions and Iron rules as collapsible blocks - one row per folder
// with its kind and the team chip, one row per session with its role and
// status; a team member's worktree is removed with its team only.
void TestProjectsDialog::projectPageHasHeaderAndCollapsibleBlocks()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-blocks-test");QCoreApplication::setApplicationName("projects");clearSettings();
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;const auto project=projects.createGroup("Orbit platform");
    projects.addFolder(project,"workstation","/work/orbit");
    projects.addFolder(project,"workstation","/work/orbit-team-528aebfe-backend");
    projects.addFolder(project,"build-01","/srv/orbit");
    ProjectsDialog dialog(&client,nullptr,true,&projects);
    dialog.setSessionsProvider([](const QString &) {
        QList<ProjectsDialog::ProjectSession> rows;
        rows << ProjectsDialog::ProjectSession{"", "claude/orbit/checkout", "checkout", "claude", "Lead", "workstation", "feat/checkout-v2", "Working", "active", "team-1"};
        rows << ProjectsDialog::ProjectSession{"", "dsh/orbit/backend", "Backend", "dsh", "Backend", "workstation", "team/528aebfe/backend", "Needs input", "warn", "team-1"};
        rows << ProjectsDialog::ProjectSession{"build-01", "codex/orbit/search", "search", "codex", "", "build-01", "feat/search-index", "Ready", "muted", ""};
        return rows;
    });
    QSignalSpy newTeam(&dialog,&ProjectsDialog::newTeamRequested);
    dialog.resize(1400,1000);dialog.selectProject(project);dialog.show();
    QCOMPARE(dialog.findChild<QLabel *>("projectHeaderName")->text(),QString("Orbit platform"));
    QCOMPARE(dialog.findChild<QLabel *>("projectSummary")->text(),QString("3 sessions · 2 in teams · 1 team"));
    // One row per folder: its kind, and the team chip for the member's worktree.
    auto *table=dialog.findChild<QTableWidget *>("projectFolders");QCOMPARE(table->columnCount(),4);QCOMPARE(table->rowCount(),3);
    QMap<QString,QString> kinds;QMap<QString,bool> chips;
    for(int row=0;row<table->rowCount();++row){
        kinds.insert(table->item(row,2)->text(),table->item(row,3)->text());
        chips.insert(table->item(row,2)->text(),table->item(row,0)->data(Qt::UserRole+1).toBool());
    }
    QCOMPARE(kinds.value("/work/orbit"),QString("Main checkout"));
    // No session runs there: worktree or clone is the member agent's choice.
    QCOMPARE(kinds.value("/work/orbit-team-528aebfe-backend"),QString("Team folder · team/528aebfe/backend"));
    QCOMPARE(kinds.value("/srv/orbit"),QString("Clone"));
    QVERIFY(chips.value("/work/orbit-team-528aebfe-backend"));QVERIFY(!chips.value("/work/orbit"));
    // The blocks with their counts.
    auto *folders=dialog.findChild<QWidget *>("projectFoldersBlock");QVERIFY(folders);
    QCOMPARE(folders->findChild<QLabel *>("collapsibleNote")->text(),QString("3"));
    auto *sessionsBlock=dialog.findChild<QWidget *>("projectSessionsBlock");QVERIFY(sessionsBlock);
    QCOMPARE(sessionsBlock->findChild<QLabel *>("collapsibleNote")->text(),QString("3 sessions · 2 in teams · 1 team"));
    QVERIFY(dialog.findChild<QWidget *>("projectRulesBlock"));QVERIFY(dialog.findChild<QWidget *>("projectIronRules"));
    // One row per session: name and role, machine, branch, status.
    auto *sessions=dialog.findChild<QTableWidget *>("projectSessions");QCOMPARE(sessions->rowCount(),3);
    QCOMPARE(sessions->item(1,0)->text(),QString("Backend"));QCOMPARE(sessions->item(1,0)->data(Qt::UserRole+2).toString(),QString("Backend"));
    QCOMPARE(sessions->item(1,0)->data(Qt::UserRole+3).toString(),QString("dsh"));
    QCOMPARE(sessions->item(1,1)->text(),QString("workstation"));QCOMPARE(sessions->item(1,2)->text(),QString("team/528aebfe/backend"));
    QCOMPARE(sessions->item(1,3)->text(),QString("Needs input"));QCOMPARE(sessions->item(1,3)->data(Qt::UserRole+1).toString(),QString("warn"));
    // A team member's worktree goes with its team.
    for(int row=0;row<table->rowCount();++row)if(table->item(row,3)->text().startsWith("Team folder"))table->setCurrentCell(row,0);
    dialog.findChild<QPushButton *>("removeProjectFolder")->click();
    QCOMPARE(projects.group(project)->folders.size(),3);
    QCOMPARE(dialog.findChild<QLabel *>("projectsStatus")->text(),QString("Team folders are removed with the team"));
    // New team… asks for this project; New session… is the primary action.
    dialog.findChild<QPushButton *>("projectNewTeam")->click();
    QCOMPARE(newTeam.size(),1);QCOMPARE(newTeam.first().first().toString(),project);
    QVERIFY(dialog.findChild<QPushButton *>("newProjectFolderSession")->property("pagePrimary").toBool());
    table->setCurrentCell(0,0);
    PageShots::shot(dialog,true,"ux-7-projects-dark");
    PageShots::shot(dialog,false,"ux-7-projects-light");
    // A block folds to its header and remembers it.
    auto *foldersCard=dialog.findChild<QWidget *>("projectFoldersCard"),*sessionsCard=dialog.findChild<QWidget *>("projectSessionsCard");
    auto *foldersHeader=folders->findChild<QPushButton *>("collapsibleHeader");
    // #646: the card is as tall as its rows and actions, never the page's empty height.
    // The selection above changed the splitter's height: the page lays out
    // again on the event loop (without shots nothing waited for it, #649).
    auto *split=dialog.findChild<QSplitter *>("projectFolderSplitter");
    const auto fits=[&]{return foldersCard->height()<=split->height()+foldersHeader->height()+40;};
    QTRY_VERIFY2(fits(),qPrintable(QStringLiteral("card %1, splitter %2").arg(foldersCard->height()).arg(split->height())));
    const int open=foldersCard->height();
    QVERIFY(table->height()<=table->horizontalHeader()->height()+3*table->verticalHeader()->defaultSectionSize()+8);
    foldersHeader->click();
    QVERIFY(!folders->findChild<QWidget *>("collapsibleBody")->isVisible());
    // QA #399 (3): the page lays out again - the card shrinks to its header,
    // which stays at its top, and the Sessions card follows right under it.
    QTRY_VERIFY2(foldersCard->height()<=foldersHeader->height()+40,qPrintable(QString::number(foldersCard->height())));
    QVERIFY(foldersHeader->mapTo(foldersCard,QPoint()).y()<=20);
    QVERIFY(sessionsCard->y()>foldersCard->geometry().bottom());QVERIFY(sessionsCard->y()-foldersCard->geometry().bottom()<=20);
    PageShots::shot(dialog,true,"ux-7-projects-collapsed-dark");
    PageShots::shot(dialog,false,"ux-7-projects-collapsed-light");
    foldersHeader->click();
    QTRY_VERIFY(qAbs(foldersCard->height()-open)<=8);
}

namespace {
SessionInfo folderSession(const QString &name,const QString &cwd,const QString &root,const QString &branch,bool worktree)
{
    SessionInfo s;s.name=name;s.cwd=cwd;s.gitRoot=root;s.gitBranch=branch;s.gitWorktree=worktree;return s;
}
QString folderKindAt(QTableWidget *table,const QString &path,bool *team=nullptr)
{
    for(int row=0;row<table->rowCount();++row)if(table->item(row,2)->text()==path){
        if(team)*team=table->item(row,0)->data(Qt::UserRole+1).toBool();
        return table->item(row,3)->text();
    }
    return {};
}
void selectFolder(QTableWidget *table,const QString &path)
{
    for(int row=0;row<table->rowCount();++row)if(table->item(row,2)->text()==path)table->setCurrentCell(row,0);
}
}

// #646 (spec 7): the page as the prototype draws it, with real data shapes -
// compact project rows with their session counts, the ghost header actions,
// rename on the name, folder rows with the team chip, label-colour machine
// tags (an unreachable one faded), bold mono folders and kinds with their
// branches, session rows with the agent, role, branch and status, and Iron
// rules with their count and hint. Every action stays.
void TestProjectsDialog::projectPageMatchesThePrototype()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-prototype-test");QCoreApplication::setApplicationName("projects");clearSettings();
    // #651 QA: a catalog for the selected folder, so the shots show the worktree overview.
    const auto script=directory.filePath("hgs");
    QVERIFY(FakeCli::install(script,R"PY(#!/usr/bin/env python3
import json,sys,time
a=sys.argv[1:]
if a and a[0].startswith('@'):a.pop(0)
if a[:1]==['worktrees']:
 p=a[a.index('--path')+1]
 print(json.dumps({'state':'ok','path':p,'selected_root':p,'common_dir':p+'/.git','sampled_at':time.time(),
  'worktrees':[{'path':p,'kind':'main','branch':'main','available':True},
               {'path':p+'/.worktrees/backend','kind':'linked','branch':'team/checkout-v2/backend','available':True}]}))
elif a[:2]==['project','ls']:print('[]')
else:print('{}')
)PY"));
    HgsClient client(FakeCli::program(script));SessionOrganization projects;
    const auto orbit=projects.createGroup("Orbit platform");projects.setColor(orbit,"#2b97ff");
    const auto infra=projects.createGroup("Infrastructure");projects.setColor(infra,"#ff9234");
    const auto wallet=projects.createGroup("Mobile wallet and the long-named partner integration");projects.setColor(wallet,"#a96bf5");
    const auto general=projects.defaultProject();QVERIFY(projects.group(general));
    for(const auto &machine:{"workstation","macbook","build-01","build-02"})projects.markImported(machine);
    const auto win=MachinePlatform::Style::Windows,posix=MachinePlatform::Style::Posix;
    projects.addFolder(orbit,"workstation","D:/work/orbit","Orbit platform",win);
    projects.addFolder(orbit,"workstation","D:/work/orbit/.worktrees/backend","Team · Backend",win);
    projects.addFolder(orbit,"macbook","/Users/user/code/orbit/.worktrees/frontend","Team · Frontend",posix);
    projects.addFolder(orbit,"workstation","D:/work/orbit-design-assets","Design assets",win);
    projects.addFolder(orbit,"build-01","/srv/orbit","build-01 clone",posix);
    projects.addFolder(orbit,"build-01","/srv/builds/orbit/services/payments-gateway/releases/2026-10/checkout-v2-hotfix-rounding","Payments hotfix checkout",posix);
    projects.addFolder(orbit,"build-02","/srv/orbit","build-02 clone",posix);
    projects.addFolder(orbit,"macbook","/Users/user/code/orbit","MacBook clone",posix);
    projects.addFolder(infra,"build-01","/srv/infra",{},posix);
    IronRules::setRules(orbit,{"Use pnpm for web and cargo for services.","Never log card numbers or provider tokens."});
    FleetState fleet;
    BoxState ws;ws.host="workstation";ws.ok=true;ws.localSessions=true;ws.metrics=QJsonObject{{"platform","windows"}};
    ws.sessions={folderSession("claude/orbit/checkout","D:/work/orbit","D:/work/orbit","main",false),
                 folderSession("dsh/orbit/backend","D:/work/orbit/.worktrees/backend","D:/work/orbit/.worktrees/backend","team/checkout-v2/backend",true),
                 folderSession("claude/orbit/design","D:/work/orbit-design-assets",{},{},false)};
    fleet.setLocal(ws,1);
    BoxState mac;mac.host="macbook";mac.ok=true;mac.metrics=QJsonObject{{"platform","macos"}};
    mac.sessions={folderSession("codex/orbit/frontend","/Users/user/code/orbit/.worktrees/frontend","/Users/user/code/orbit/.worktrees/frontend","team/checkout-v2/frontend",true),
                  folderSession("claude/orbit/ios","/Users/user/code/orbit","/Users/user/code/orbit","main",false)};
    fleet.setPeer(mac,1);
    BoxState b1;b1.host="build-01";b1.ok=true;b1.metrics=QJsonObject{{"platform","linux"}};
    b1.sessions={folderSession("kimi/orbit/tester","/srv/orbit","/srv/orbit","main",false),
                 folderSession("opencode/orbit/hotfix","/srv/builds/orbit/services/payments-gateway/releases/2026-10/checkout-v2-hotfix-rounding",
                               "/srv/builds/orbit/services/payments-gateway/releases/2026-10/checkout-v2-hotfix-rounding","hotfix/payments-rounding",false)};
    fleet.setPeer(b1,1);
    // build-02 cannot be reached.
    BoxState b2;b2.host="build-02";b2.ok=false;b2.metrics=QJsonObject{{"platform","linux"}};
    b2.sessions={folderSession("dsh/orbit/review","/srv/orbit","/srv/orbit","release/2.4",false)};
    fleet.setPeer(b2,1);
    ProjectsDialog dialog(&client,nullptr,true,&projects);
    dialog.setFleet(fleet);
    dialog.setSessionsProvider([](const QString &project) {
        QList<ProjectsDialog::ProjectSession> rows;
        if(project.isEmpty())return rows;
        const auto row=[&rows](const QString &host,const QString &name,const QString &title,const QString &agent,const QString &role,const QString &machine,
                              const QString &branch,const QString &status,const QString &tone,const QString &team,const QString &cwd,bool teamFolder){
            rows<<ProjectsDialog::ProjectSession{host,name,title,agent,role,machine,branch,status,tone,team,cwd,teamFolder,qint64(1000-rows.size())};
        };
        row("","claude/orbit/checkout","checkout","claude","Lead","workstation","feat/checkout-v2","Ready","muted","team-1","D:/work/orbit",false);
        row("","dsh/orbit/backend","Backend","dsh","Backend","workstation","team/checkout-v2/backend","Needs approval","warn","team-1","D:/work/orbit/.worktrees/backend",true);
        row("macbook","codex/orbit/frontend","Frontend","codex","Frontend","macbook","team/checkout-v2/frontend","Ready","muted","team-1","/Users/user/code/orbit/.worktrees/frontend",true);
        row("build-01","kimi/orbit/tester","Tester","kimi","Tester","build-01","main","Working","active","team-1","/srv/orbit",false);
        row("","codex/orbit/search","search","codex","","workstation","feat/search-index","Working","active","","D:/work/orbit",false);
        row("build-02","dsh/orbit/review","review","dsh","","build-02","release/2.4","Offline","offline","","/srv/orbit",false);
        row("build-01","opencode/orbit/hotfix","payments-hotfix-with-a-long-session-name","opencode","","build-01","hotfix/payments-rounding","New reply","warn","",
            "/srv/builds/orbit/services/payments-gateway/releases/2026-10/checkout-v2-hotfix-rounding",false);
        row("macbook","claude/orbit/ios","ios-shell","claude","","macbook","feat/ios-wallet","Paused","paused","","/Users/user/code/orbit",false);
        return rows;
    });
    dialog.setSessionCountsProvider([orbit,infra,general]{return QHash<QString,int>{{orbit,8},{infra,1},{general,3}};});
    QSignalSpy newTeam(&dialog,&ProjectsDialog::newTeamRequested),swarm(&dialog,&ProjectsDialog::swarmRequested);
    QSignalSpy launch(&dialog,&ProjectsDialog::newSessionRequested);
    dialog.resize(1280,1000);dialog.selectProject(orbit);dialog.show();
    // The project list: compact rows with their session counts, + Project and the hint.
    auto *list=dialog.findChild<QListWidget *>("logicalProjects");QCOMPARE(list->count(),4);
    QCOMPARE(list->item(0)->data(Qt::UserRole+4).toInt(),8);QCOMPARE(list->item(2)->data(Qt::UserRole+4).toInt(),0);
    QVERIFY(list->item(3)->data(Qt::UserRole+3).toBool());   // General is the default
    QCOMPARE(list->visualItemRect(list->item(0)).height(),38);
    QVERIFY(list->height()<=4*38+10);
    QCOMPARE(dialog.findChild<QLabel *>("projectsHeading")->text(),QString("Projects"));
    QCOMPARE(dialog.findChild<QPushButton *>("newProject")->text(),QString("Project"));
    QVERIFY(dialog.findChild<QLabel *>("projectListHint")->text().startsWith("Projects group sessions"));
    // The header: ghost Color…, Worktrees… and Swarm…, New team… and the primary New session….
    for(const char *name:{"projectColor","projectWorktrees","projectSwarm"}){
        auto *button=dialog.findChild<QPushButton *>(name);QVERIFY2(button&&button->property("pageGhost").toBool()&&button->property("pageSm").toBool(),name);
    }
    QVERIFY(!dialog.findChild<QPushButton *>("projectNewTeam")->property("pageGhost").toBool());
    QVERIFY(dialog.findChild<QPushButton *>("newProjectFolderSession")->property("pagePrimary").toBool());
    QCOMPARE(dialog.findChild<QLabel *>("projectSummary")->text(),QString("8 sessions · 4 in teams · 1 team"));
    QCOMPARE(dialog.findChild<QLabel *>("projectId")->text(),orbit);
    // No name field: the name renames in place.
    auto *name=dialog.findChild<QLineEdit *>("projectName");QVERIFY(name->isHidden());
    // Folders: one row each, the team chip, the machines, the kinds with their branches.
    auto *table=dialog.findChild<QTableWidget *>("projectFolders");QCOMPARE(table->rowCount(),8);
    QCOMPARE(folderKindAt(table,"D:\\work\\orbit"),QString("Main checkout · main"));
    bool team=false;QCOMPARE(folderKindAt(table,"D:\\work\\orbit\\.worktrees\\backend",&team),QString("Worktree · team/checkout-v2/backend"));QVERIFY(team);
    QCOMPARE(folderKindAt(table,"/Users/user/code/orbit/.worktrees/frontend",&team),QString("Worktree · team/checkout-v2/frontend"));QVERIFY(team);
    QCOMPARE(folderKindAt(table,"D:\\work\\orbit-design-assets"),QString("Folder"));
    QCOMPARE(folderKindAt(table,"/srv/builds/orbit/services/payments-gateway/releases/2026-10/checkout-v2-hotfix-rounding"),QString("Clone · hotfix/payments-rounding"));
    int offline=0;for(int row=0;row<table->rowCount();++row)if(table->item(row,1)->data(Qt::UserRole+2).toBool()){
        ++offline;QCOMPARE(table->item(row,1)->text(),QString("build-02"));QCOMPARE(table->item(row,1)->toolTip(),QString("build-02 is offline"));
        QCOMPARE(table->item(row,3)->text(),QString("Clone"));   // no live session tells its branch
    }
    QCOMPARE(offline,1);
    QVERIFY(table->item(0,2)->font().bold());
    // The table keeps its rows' height; nothing stretches it.
    QCOMPARE(table->height(),table->horizontalHeader()->height()+8*table->verticalHeader()->defaultSectionSize()+8);
    // Sessions: agent, bold name, role, machine, branch, status.
    auto *sessions=dialog.findChild<QTableWidget *>("projectSessions");QCOMPARE(sessions->rowCount(),8);
    QCOMPARE(sessions->item(1,0)->text(),QString("Backend"));QCOMPARE(sessions->item(1,0)->data(Qt::UserRole+2).toString(),QString("Backend"));
    QCOMPARE(sessions->item(1,0)->data(Qt::UserRole+3).toString(),QString("dsh"));QVERIFY(sessions->item(1,0)->font().bold());
    QCOMPARE(sessions->item(5,3)->data(Qt::UserRole+1).toString(),QString("offline"));QVERIFY(sessions->item(5,1)->data(Qt::UserRole+2).toBool());
    QCOMPARE(sessions->item(7,3)->data(Qt::UserRole+1).toString(),QString("paused"));
    // Iron rules: their count and what they are for.
    auto *rules=dialog.findChild<QWidget *>("projectRulesBlock");
    QCOMPARE(rules->findChild<QLabel *>("collapsibleNote")->text(),QString("2"));
    QVERIFY(dialog.findChild<QLabel *>("projectRulesHint")->text().startsWith("Sent at the start of every message"));
    // The first folder selected, as the prototype shows the page.
    selectFolder(table,"D:\\work\\orbit");
    PageShots::shot(dialog,true,"pm-projects-dark");
    PageShots::shot(dialog,false,"pm-projects-light");
    // Actions: New team… and Swarm… ask for them; New session… starts in the
    // selected folder and refuses one on the unreachable machine.
    dialog.findChild<QPushButton *>("projectNewTeam")->click();QCOMPARE(newTeam.size(),1);QCOMPARE(newTeam.first().first().toString(),orbit);
    dialog.findChild<QPushButton *>("projectSwarm")->click();QCOMPARE(swarm.size(),1);
    auto *create=dialog.findChild<QPushButton *>("newProjectFolderSession");
    selectFolder(table,"/srv/builds/orbit/services/payments-gateway/releases/2026-10/checkout-v2-hotfix-rounding");
    QVERIFY(create->isEnabled());create->click();QCOMPARE(launch.size(),1);QCOMPARE(launch.first().at(0).toString(),orbit);
    QCOMPARE(launch.first().at(1).toString(),QString("build-01"));
    for(int row=0;row<table->rowCount();++row)if(table->item(row,1)->data(Qt::UserRole+2).toBool())table->setCurrentCell(row,0);
    QVERIFY(!create->isEnabled());QCOMPARE(create->toolTip(),QString("Reconnect build-02 to start a session in this folder."));
    // Rename: a click on the name opens it in place; Escape keeps the old
    // name, Enter the new one.
    auto *header=dialog.findChild<QLabel *>("projectHeaderName");
    QTest::mouseClick(header,Qt::LeftButton);QVERIFY(!name->isHidden());QVERIFY(header->isHidden());QCOMPARE(name->text(),QString("Orbit platform"));
    selectFolder(table,"D:\\work\\orbit");
    PageShots::shot(dialog,true,"pm-projects-rename-dark");
    PageShots::shot(dialog,false,"pm-projects-rename-light");
    name->setFocus();QTest::keyClicks(name,"x");QTest::keyClick(name,Qt::Key_Escape);
    QTRY_VERIFY(name->isHidden());QVERIFY(!header->isHidden());QCOMPARE(projects.group(orbit)->name,QString("Orbit platform"));
    QTest::mouseClick(header,Qt::LeftButton);name->selectAll();QTest::keyClicks(name,"Orbit");QTest::keyClick(name,Qt::Key_Return);
    QTRY_VERIFY(name->isHidden());QCOMPARE(projects.group(orbit)->name,QString("Orbit"));QCOMPARE(header->text(),QString("Orbit"));
    QCOMPARE(list->item(0)->text(),QString("Orbit"));
    projects.renameGroup(orbit,"Orbit platform");dialog.refresh();
    // Enter renamed and nothing else: no block folded.
    for(const char *block:{"projectFoldersBlock","projectSessionsBlock","projectRulesBlock"})
        QVERIFY2(dialog.findChild<QWidget *>(block)->findChild<QWidget *>("collapsibleBody")->isVisible(),block);
    // Sessions and Iron rules folded, as the prototype's second shot.
    for(const char *block:{"projectSessionsBlock","projectRulesBlock"})dialog.findChild<QWidget *>(block)->findChild<QPushButton *>("collapsibleHeader")->click();
    selectFolder(table,"D:\\work\\orbit");
    PageShots::shot(dialog,true,"pm-projects-collapsed-dark");
    PageShots::shot(dialog,false,"pm-projects-collapsed-light");
    for(const char *block:{"projectSessionsBlock","projectRulesBlock"})dialog.findChild<QWidget *>(block)->findChild<QPushButton *>("collapsibleHeader")->click();
}

// #700 (N4, L1): the page's message is in view whatever the details'
// scroll does, and only a refusal is red: a later ordinary message is not.
void TestProjectsDialog::pageMessagesStayInViewAndOnlyRefusalsAreRed()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-status-test");QCoreApplication::setApplicationName("projects");clearSettings();
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;const auto project=projects.createGroup("Orbit platform");
    projects.addFolder(project,"workstation","/work/orbit");
    projects.addFolder(project,"workstation","/work/orbit-team-528aebfe-backend");
    ProjectsDialog dialog(&client,nullptr,true,&projects);
    // Fifty sessions: the details scroll far past the window.
    dialog.setSessionsProvider([](const QString &) {
        QList<ProjectsDialog::ProjectSession> rows;
        for(int i=0;i<50;++i)rows<<ProjectsDialog::ProjectSession{"",QStringLiteral("codex/orbit/s%1").arg(i),QStringLiteral("s%1").arg(i),"codex","","workstation","main","Ready","muted","",{},false,i};
        return rows;
    });
    dialog.resize(1280,900);dialog.selectProject(project);dialog.show();
    auto *table=dialog.findChild<QTableWidget *>("projectFolders");
    for(int row=0;row<table->rowCount();++row)if(table->item(row,3)->text().startsWith("Team folder"))table->setCurrentCell(row,0);
    dialog.findChild<QPushButton *>("removeProjectFolder")->click();
    auto *status=dialog.findChild<QLabel *>("projectsStatus");
    QCOMPARE(status->text(),QString("Team folders are removed with the team"));
    QTRY_VERIFY(status->isVisible());
    // Outside the details' scroll, at the page's foot: in view without scrolling.
    QVERIFY(!dialog.findChild<QWidget *>("projectDetailsScroll")->isAncestorOf(status));
    const QRect shown(status->mapTo(&dialog,QPoint()),status->size());
    QVERIFY2(dialog.rect().contains(shown),qPrintable(QStringLiteral("%1,%2 %3x%4").arg(shown.x()).arg(shown.y()).arg(shown.width()).arg(shown.height())));
    QVERIFY(status->property("pageError").toBool());
    PageShots::shot(dialog,true,"pm700-refusal-dark");
    PageShots::shot(dialog,false,"pm700-refusal-light");
    // A later ordinary message is not red.
    dialog.findChild<QCheckBox *>("defaultProject")->click();
    QTRY_COMPARE(status->text(),QString("Saved"));
    QVERIFY(!status->property("pageError").toBool());
    PageShots::shot(dialog,true,"pm700-saved-dark");
    PageShots::shot(dialog,false,"pm700-saved-light");
}

// #700 (L2): a rename renames the project it started on, whatever the list
// selects later, and is cancelled when that project disappears.
void TestProjectsDialog::renameKeepsItsProjectOrIsCancelled()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-rename-test");QCoreApplication::setApplicationName("projects");clearSettings();
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;
    const auto alpha=projects.createGroup("Alpha"),beta=projects.createGroup("Beta");
    ProjectsDialog dialog(&client,nullptr,true,&projects);dialog.resize(1200,800);dialog.selectProject(alpha);dialog.show();
    auto *header=dialog.findChild<QLabel *>("projectHeaderName");auto *name=dialog.findChild<QLineEdit *>("projectName");
    // The list is rebuilt under the rename (a swarm update reorders it): Enter still renames Alpha.
    QTest::mouseClick(header,Qt::LeftButton);QVERIFY(!name->isHidden());
    name->selectAll();QTest::keyClicks(name,"Alpha renamed");
    projects.moveGroup(beta,alpha);dialog.refresh();
    QVERIFY(!name->isHidden());
    QTest::keyClick(name,Qt::Key_Return);
    QTRY_VERIFY(name->isHidden());
    QCOMPARE(projects.group(alpha)->name,QString("Alpha renamed"));QCOMPARE(projects.group(beta)->name,QString("Beta"));
    // Alpha disappears while it is renamed: the rename is cancelled, Beta keeps its name.
    dialog.selectProject(alpha);
    QTest::mouseClick(header,Qt::LeftButton);QVERIFY(!name->isHidden());
    name->selectAll();QTest::keyClicks(name,"Never applied");
    QVERIFY(projects.removeGroup(alpha,beta));dialog.refresh();
    QTRY_VERIFY(name->isHidden());QVERIFY(!header->isHidden());
    QCOMPARE(projects.group(beta)->name,QString("Beta"));
    QCOMPARE(header->text(),QString("Beta"));
    QTest::keyClick(name,Qt::Key_Return);
    QCOMPARE(projects.group(beta)->name,QString("Beta"));
}

// #700 (L3): a narrow window keeps the Kind column reachable: Name and Kind
// shrink first, then the table scrolls sideways; the last row stays in view.
void TestProjectsDialog::narrowWindowKeepsTheKindColumnReachable()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-narrow-test");QCoreApplication::setApplicationName("projects");clearSettings();
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;const auto project=projects.createGroup("Orbit platform");
    projects.addFolder(project,"workstation","/work/orbit");
    projects.addFolder(project,"build-01","/srv/builds/orbit/services/payments-gateway/releases/2026-10/checkout-v2-hotfix-rounding");
    ProjectsDialog dialog(&client,nullptr,true,&projects);dialog.selectProject(project);
    auto *table=dialog.findChild<QTableWidget *>("projectFolders");
    const auto reachable=[&]{
        const int end=table->horizontalHeader()->sectionViewportPosition(3)+table->columnWidth(3);
        auto *bar=table->horizontalScrollBar();
        return end<=table->viewport()->width()+1||(bar->isVisible()&&bar->maximum()>0&&bar->maximum()>=end-table->viewport()->width()-1);
    };
    // Wide: every column at its width.
    dialog.resize(1500,900);dialog.show();
    QTRY_COMPARE(table->columnWidth(0),240);QTRY_COMPARE(table->columnWidth(3),220);QVERIFY(reachable());
    // Narrower: Name and Kind give up room before anything is cut.
    dialog.resize(1050,900);
    QTRY_VERIFY(table->columnWidth(0)<240&&table->columnWidth(3)<220);
    QTRY_VERIFY(reachable());
    // Narrow, with a wide project list beside the details: the details
    // scroll sideways (and the table too, if it is still too narrow), so
    // scrolling brings the whole Kind column into view; no row is covered.
    dialog.resize(900,900);QTest::qWait(30);
    auto *pages=dialog.findChild<QSplitter *>("projectsSplit");QVERIFY(pages);pages->setSizes({520,380});
    auto *details=dialog.findChild<QScrollArea *>("projectDetailsScroll");QVERIFY(details);
    QTRY_VERIFY2(details->viewport()->width()<420,qPrintable(QStringLiteral("split %1 %2 details %3 viewport %4 content %5").arg(pages->sizes()[0]).arg(pages->sizes()[1]).arg(details->width()).arg(details->viewport()->width()).arg(details->widget()->width())));
    QTRY_VERIFY(details->horizontalScrollBar()->isVisible());
    details->horizontalScrollBar()->setValue(details->horizontalScrollBar()->maximum());
    if(table->horizontalScrollBar()->isVisible())table->horizontalScrollBar()->setValue(table->horizontalScrollBar()->maximum());
    QTest::qWait(30);
    const int kindEnd=table->horizontalHeader()->sectionViewportPosition(3)+table->columnWidth(3);
    QVERIFY2(kindEnd<=table->viewport()->width()+1,qPrintable(QStringLiteral("%1 of %2").arg(kindEnd).arg(table->viewport()->width())));
    const QRect kind(table->viewport()->mapTo(details->viewport(),QPoint(table->horizontalHeader()->sectionViewportPosition(3),0)),QSize(table->columnWidth(3),1));
    QVERIFY2(details->viewport()->rect().contains(kind.topLeft())&&kind.right()<=details->viewport()->width(),
             qPrintable(QStringLiteral("Kind %1..%2 in %3").arg(kind.left()).arg(kind.right()).arg(details->viewport()->width())));
    QTRY_VERIFY(table->viewport()->height()>=table->rowCount()*table->verticalHeader()->defaultSectionSize());
    PageShots::shot(dialog,true,"pm700-narrow-dark");
    PageShots::shot(dialog,false,"pm700-narrow-light");
}

namespace {
// Grabs the modal dialog named `name` in the page's theme, then answers it.
// `fill` sets its fields before the shot. Gives up (the test then fails on a
// dialog left open) after about three seconds.
void grabDialog(const QString &name,const QString &shot,bool dark,const std::function<void(QDialog *)> &answer,
                const std::function<void(QDialog *)> &fill={},int tries=300)
{
    QTimer::singleShot(10,qApp,[=]{
        auto *dialog=qobject_cast<QDialog *>(QApplication::activeModalWidget());
        if(!dialog||dialog->objectName()!=name){
            if(tries>0)grabDialog(name,shot,dark,answer,fill,tries-1);
            else if(dialog)dialog->reject();
            return;
        }
        if(fill)fill(dialog);
        if(const auto dir=qEnvironmentVariable("ZERUS_TEAM_SHOTS");!dir.isEmpty()){
            dialog->setStyleSheet(PageShots::windowSheet(dark,dialog->objectName())+dialog->styleSheet());
            QTest::qWait(120);dialog->grab().save(QDir(dir).filePath(shot+QStringLiteral(".png")));
        }
        answer(dialog);
    });
}
}

// #651 QA: the dialogs of the Projects page have shots in both themes - add
// a folder, remove one (its confirmation), clone a repository (the computer,
// then the form) - and the page shows the worktree overview of a real
// catalog, not the fixture's error.
void TestProjectsDialog::folderDialogsHaveShots()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-dialogs-test");QCoreApplication::setApplicationName("projects");clearSettings();
    const auto script=directory.filePath("hgs");
    QVERIFY(FakeCli::install(script,R"PY(#!/usr/bin/env python3
import json,sys,time
a=sys.argv[1:]
if a and a[0].startswith('@'):a.pop(0)
def arg(k):return a[a.index(k)+1]
if a==['--help']:print('repository-clone-v1')
elif a[:1]==['worktrees']:
 p=arg('--path')
 print(json.dumps({'state':'ok','path':p,'selected_root':p,'common_dir':p+'/.git','sampled_at':time.time(),
  'worktrees':[{'path':p,'kind':'main','branch':'main','available':True},
               {'path':p+'/.worktrees/backend','kind':'linked','branch':'team/checkout-v2/backend','available':True},
               {'path':p+'/.worktrees/frontend','kind':'linked','branch':'team/checkout-v2/frontend','available':True}]}))
elif a[:2]==['project','ls']:print('[]')
else:print('{}')
)PY"));
    HgsClient client(FakeCli::program(script));SessionOrganization projects;const auto project=projects.createGroup("Orbit platform");
    projects.markImported("workstation");projects.markImported("build-01");
    projects.addFolder(project,"workstation","/work/orbit","Orbit platform");
    projects.addFolder(project,"build-01","/srv/orbit","build-01 clone");
    ProjectsDialog dialog(&client,nullptr,true,&projects);
    FleetState fleet;BoxState ws;ws.host="workstation";ws.ok=true;ws.localSessions=true;ws.metrics=QJsonObject{{"platform","linux"}};fleet.setLocal(ws,1);
    BoxState b1;b1.host="build-01";b1.ok=true;b1.metrics=QJsonObject{{"platform","linux"}};fleet.setPeer(b1,1);
    dialog.setFleet(fleet);dialog.resize(1280,1000);dialog.selectProject(project);dialog.show();
    auto *table=dialog.findChild<QTableWidget *>("projectFolders");
    for(int row=0;row<table->rowCount();++row)if(table->item(row,2)->text()=="/work/orbit")table->setCurrentCell(row,0);
    // The worktree overview of the selected folder, from the catalog.
    auto *catalog=dialog.findChild<QTreeWidget *>("worktreeCatalog");QVERIFY(catalog);
    auto *catalogStatus=dialog.findChild<QLabel *>("worktreeStatus");QVERIFY(catalogStatus);
    QTRY_VERIFY2(catalogStatus->text().startsWith("Existing working folders on workstation"),qPrintable(catalogStatus->text()));
    QTRY_VERIFY(catalog->topLevelItemCount()>=3);
    for(const bool dark:{true,false}){
        const QString theme=dark?QStringLiteral("dark"):QStringLiteral("light");
        PageShots::shot(dialog,dark,"pm700-worktrees-"+theme);
        dialog.setTheme(dark);
        // Add a folder.
        grabDialog("projectFolderDialog","pm700-add-folder-"+theme,dark,[](QDialog *d){d->reject();});
        dialog.findChild<QPushButton *>("addProjectFolder")->click();
        // Remove a folder: its confirmation.
        for(int row=0;row<table->rowCount();++row)if(table->item(row,2)->text()=="/srv/orbit")table->setCurrentCell(row,0);
        grabDialog("removeProjectFolderConfirm","pm700-remove-folder-"+theme,dark,[](QDialog *d){d->reject();});
        dialog.findChild<QPushButton *>("removeProjectFolder")->click();
        QCOMPARE(projects.group(project)->folders.size(),2);
        // Clone: the computer, then the form.
        grabDialog("cloneRepositoryComputerDialog","pm700-clone-computer-"+theme,dark,[&,theme,dark](QDialog *choice){
            grabDialog("cloneRepositoryDialog","pm700-clone-form-"+theme,dark,[](QDialog *form){form->reject();},[](QDialog *form){
                form->findChild<QLineEdit *>("cloneRepositoryUrl")->setText("https://git.example.invalid/team/orbit.git");
                form->findChild<QLineEdit *>("cloneParentFolder")->setText("/srv");
            });
            choice->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Ok)->click();
        });
        dialog.findChild<QPushButton *>("cloneProjectRepository")->click();
        QCOMPARE(projects.group(project)->folders.size(),2);
        for(int row=0;row<table->rowCount();++row)if(table->item(row,2)->text()=="/work/orbit")table->setCurrentCell(row,0);
    }
}

// N24 (#386): a folder without Git is a Folder; a team member's own folder is
// known by its generated path or by the member session running there, never
// by its branch, so a team clone keeps its chip and a person's own worktree
// on a "team/..." branch can be removed. N25: Remove folder asks first.
void TestProjectsDialog::folderKindsFollowTheFleetAndTheTeams()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-kinds-test");QCoreApplication::setApplicationName("projects");clearSettings();
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;const auto project=projects.createGroup("Orbit");projects.markImported("workstation");
    for(const auto &path:{"/work/orbit","/work/notes","/work/orbit-team-528aebfe-dsh","/work/orbit-mine","/work/orbit-member","/srv/orbit"})
        projects.addFolder(project,"workstation",path);
    BoxState ws;ws.host="workstation";ws.ok=true;ws.metrics=QJsonObject{{"platform","linux"}};
    ws.sessions={folderSession("claude/orbit/main","/work/orbit/src","/work/orbit","main",false),
                 folderSession("claude/notes/todo","/work/notes",{},{},false),
                 folderSession("dsh/orbit/dsh","/work/orbit-team-528aebfe-dsh","/work/orbit-team-528aebfe-dsh","team/528aebfe/dsh",false),
                 folderSession("codex/orbit/mine","/work/orbit-mine","/work/orbit-mine","team/experiment",true),
                 folderSession("claude/orbit/qa","/work/orbit-member","/work/orbit-member","team/528aebfe/qa",true)};
    FleetState fleet;fleet.setLocal(ws,1);
    ProjectsDialog dialog(&client,nullptr,true,&projects);
    dialog.setSessionsProvider([](const QString &){
        ProjectsDialog::ProjectSession member{"", "claude/orbit/qa", "qa", "claude", "QA", "workstation", "team/528aebfe/qa", "Working", "active", "team-1", "/work/orbit-member"};
        member.teamFolder=true;
        return QList<ProjectsDialog::ProjectSession>{member};
    });
    dialog.setFleet(fleet);dialog.resize(1400,1000);dialog.selectProject(project);dialog.show();
    auto *table=dialog.findChild<QTableWidget *>("projectFolders");QCOMPARE(table->rowCount(),6);
    bool team=true;
    QCOMPARE(folderKindAt(table,"/work/orbit",&team),QString("Main checkout · main"));QVERIFY(!team);
    QCOMPARE(folderKindAt(table,"/work/notes",&team),QString("Folder"));QVERIFY(!team);
    QCOMPARE(folderKindAt(table,"/work/orbit-team-528aebfe-dsh",&team),QString("Clone · team/528aebfe/dsh"));QVERIFY(team);
    QCOMPARE(folderKindAt(table,"/work/orbit-mine",&team),QString("Worktree · team/experiment"));QVERIFY(!team);
    QCOMPARE(folderKindAt(table,"/work/orbit-member",&team),QString("Worktree · team/528aebfe/qa"));QVERIFY(team);
    QCOMPARE(folderKindAt(table,"/srv/orbit",&team),QString("Clone"));QVERIFY(!team);
    PageShots::shot(dialog,true,"ux-7-projects-kinds-dark");
    // The team clone goes with its team, running session or not.
    selectFolder(table,"/work/orbit-team-528aebfe-dsh");
    dialog.findChild<QPushButton *>("removeProjectFolder")->click();
    QCOMPARE(projects.group(project)->folders.size(),6);
    QCOMPARE(dialog.findChild<QLabel *>("projectsStatus")->text(),QString("Team folders are removed with the team"));
    // A person's own worktree on a "team/..." branch: Remove folder asks;
    // Cancel keeps it, Remove folder removes it (the files stay).
    QString prompt;
    bool answerFound = false;
    const auto answer=[&](bool remove,bool *asked){
        QTimer::singleShot(0,&dialog,[&dialog,remove,asked,&prompt,&answerFound]{
            auto *confirm=dialog.findChild<QMessageBox *>("removeProjectFolderConfirm");
            if (!confirm) { if (auto *modal=qobject_cast<QDialog *>(QApplication::activeModalWidget())) modal->reject(); return; }
            *asked=true; prompt=confirm->text();
            auto *button=remove ? confirm->findChild<QPushButton *>("removeProjectFolderConfirmButton") : confirm->button(QMessageBox::Cancel);
            answerFound = button != nullptr;
            if (button) button->click(); else confirm->reject();
        });
    };
    const auto checkPrompt=[&]{
        QVERIFY(answerFound);
        QVERIFY(prompt.contains("Files stay on disk."));
        QVERIFY(prompt.contains("/work/orbit-mine"));
    };
    bool asked=false;
    selectFolder(table,"/work/orbit-mine");answer(false,&asked);
    dialog.findChild<QPushButton *>("removeProjectFolder")->click();
    QVERIFY(asked);checkPrompt();QCOMPARE(projects.group(project)->folders.size(),6);
    // The folder menu asks the same question.
    asked=false;selectFolder(table,"/work/orbit-mine");answer(false,&asked);
    emit table->customContextMenuRequested(table->visualItemRect(table->currentItem()).center());
    QPointer<QMenu> menu=dialog.findChild<QMenu *>("projectFolderMenu");QVERIFY(menu);
    menu->findChild<QAction *>("contextRemoveProjectFolder")->trigger();delete menu.data();
    QVERIFY(asked);checkPrompt();QCOMPARE(projects.group(project)->folders.size(),6);
    asked=false;selectFolder(table,"/work/orbit-mine");answer(true,&asked);
    dialog.findChild<QPushButton *>("removeProjectFolder")->click();
    QVERIFY(asked);checkPrompt();QCOMPARE(projects.group(project)->folders.size(),5);
    QVERIFY(folderKindAt(table,"/work/orbit-mine").isEmpty());
}

// N22 (#386): while the page shows, a fleet poll refreshes the Sessions rows
// and the folder kinds in place (the folder selection stays), at most once a
// second; a hidden page waits until it shows again.
void TestProjectsDialog::projectSessionsFollowTheFleetWhileShown()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-live-test");QCoreApplication::setApplicationName("projects");clearSettings();
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;const auto project=projects.createGroup("Orbit");projects.markImported("workstation");
    projects.addFolder(project,"workstation","/work/orbit");projects.addFolder(project,"workstation","/srv/orbit");
    BoxState ws;ws.host="workstation";ws.ok=true;ws.metrics=QJsonObject{{"platform","linux"}};
    ws.sessions={folderSession("claude/orbit/main","/work/orbit","/work/orbit","main",false)};
    FleetState fleet;fleet.setLocal(ws,1);
    QList<ProjectsDialog::ProjectSession> rows{{"", "claude/orbit/main", "main", "claude", "", "workstation", "main", "Working", "active", ""}};
    int asked=0;
    ProjectsDialog dialog(&client,nullptr,true,&projects);
    dialog.setSessionsProvider([&](const QString &){++asked;return rows;});
    dialog.setFleet(fleet);dialog.resize(1400,1000);dialog.selectProject(project);dialog.show();
    auto *sessions=dialog.findChild<QTableWidget *>("projectSessions");auto *folders=dialog.findChild<QTableWidget *>("projectFolders");
    QCOMPARE(sessions->rowCount(),1);QCOMPARE(sessions->item(0,3)->text(),QString("Working"));
    QCOMPARE(folderKindAt(folders,"/work/orbit"),QString("Main checkout · main"));
    selectFolder(folders,"/srv/orbit");
    // The session finished, a new one started, the branch moved on.
    rows[0].status="Ready";rows[0].tone="muted";
    rows<<ProjectsDialog::ProjectSession{"", "codex/orbit/api", "api", "codex", "", "workstation", "feat/api", "Working", "active", "", {}, false, 10};
    ws.sessions[0].gitBranch="release/2.5";fleet.setLocal(ws,2);
    QTest::qWait(1100);const int before=asked;
    for(int poll=0;poll<5;++poll)dialog.setFleet(fleet);
    QTRY_COMPARE(sessions->rowCount(),2);
    QCOMPARE(sessions->item(0,0)->data(Qt::UserRole).toString(),QString("codex/orbit/api"));
    QCOMPARE(sessions->item(1,3)->text(),QString("Ready"));
    QCOMPARE(folderKindAt(folders,"/work/orbit"),QString("Main checkout · release/2.5"));
    QCOMPARE(folders->item(folders->currentRow(),2)->text(),QString("/srv/orbit"));
    // Five polls, one refresh.
    QTest::qWait(1200);QCOMPARE(asked,before+1);
    // Hidden, the page skips the polls; shown again, it catches up.
    dialog.hide();rows.removeFirst();dialog.setFleet(fleet);QTest::qWait(1200);
    QCOMPARE(sessions->rowCount(),2);QCOMPARE(asked,before+1);
    dialog.show();
    QCOMPARE(sessions->rowCount(),1);QCOMPARE(sessions->item(0,0)->data(Qt::UserRole).toString(),QString("codex/orbit/api"));
}

// N23 (#386): the Sessions block lists the 50 newest sessions and opens the
// rest in Sessions; the folder kinds take one pass over a machine's sessions,
// so 40 folders on a machine with 10 000 sessions show at once.
void TestProjectsDialog::projectSessionRowsAreBounded()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-bounded-test");QCoreApplication::setApplicationName("projects");clearSettings();
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;const auto project=projects.createGroup("Orbit");projects.markImported("workstation");
    for(int i=0;i<40;++i)projects.addFolder(project,"workstation",QStringLiteral("/work/folder-%1").arg(i));
    BoxState ws;ws.host="workstation";ws.ok=true;ws.metrics=QJsonObject{{"platform","linux"}};
    for(int i=0;i<10000;++i)ws.sessions<<folderSession(QStringLiteral("codex/orbit/s%1").arg(i),QStringLiteral("/work/session-%1/./src/..").arg(i),QStringLiteral("/work/session-%1").arg(i),"main",false);
    ws.sessions<<folderSession("codex/orbit/last","/work/folder-39","/work/folder-39","feat/last",false);
    FleetState fleet;fleet.setLocal(ws,1);
    QList<ProjectsDialog::ProjectSession> rows;
    for(int i=0;i<120;++i)rows<<ProjectsDialog::ProjectSession{"", QStringLiteral("codex/orbit/s%1").arg(i), QStringLiteral("s%1").arg(i), "codex", "", "workstation", "main", "Ready", "muted", "", {}, false, i};
    ProjectsDialog dialog(&client,nullptr,true,&projects);
    dialog.setSessionsProvider([&rows](const QString &){return rows;});
    dialog.setFleet(fleet);dialog.resize(1400,1000);
    QElapsedTimer timer;timer.start();
    dialog.selectProject(project);dialog.setTheme(true);
    QVERIFY2(timer.elapsed()<2000,qPrintable(QString::number(timer.elapsed())));
    auto *folders=dialog.findChild<QTableWidget *>("projectFolders");QCOMPARE(folders->rowCount(),40);
    QCOMPARE(folderKindAt(folders,"/work/folder-39"),QString("Clone · feat/last"));
    QCOMPARE(folderKindAt(folders,"/work/folder-0"),QString("Main checkout"));
    dialog.show();
    auto *sessions=dialog.findChild<QTableWidget *>("projectSessions");
    QCOMPARE(sessions->rowCount(),ProjectsDialog::kSessionRows);
    QCOMPARE(sessions->item(0,0)->data(Qt::UserRole).toString(),QString("codex/orbit/s119"));
    QCOMPARE(sessions->item(49,0)->data(Qt::UserRole).toString(),QString("codex/orbit/s70"));
    QCOMPARE(dialog.findChild<QLabel *>("projectSummary")->text(),QString("120 sessions · 0 in teams · 0 teams"));
    auto *all=dialog.findChild<QPushButton *>("projectAllSessions");QVERIFY(all->isVisible());
    QCOMPARE(all->text(),QString("Show all 120 in Sessions"));
    QSignalSpy requested(&dialog,&ProjectsDialog::projectSessionsRequested);all->click();
    QCOMPARE(requested.size(),1);QCOMPARE(requested.first().first().toString(),project);
    // Fewer than 50: every row, no link.
    rows.resize(3);dialog.selectProject(project);
    QCOMPARE(sessions->rowCount(),3);QVERIFY(!all->isVisible());
}

void TestProjectsDialog::cloneAddsFolderWithoutLaunchingSession()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-clone-test");QCoreApplication::setApplicationName("projects");
    const auto script=directory.filePath("hgs");
    QVERIFY(FakeCli::install(script,R"PY(#!/usr/bin/env python3
import json,sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a.pop(0)
def arg(k):return a[a.index(k)+1]
if a==['--help']:print('repository-clone-v1')
elif a[:2]==['repository','check']:print(json.dumps({'status':'ready','parent':'/workspace','path':'/workspace/'+arg('--name'),'name':arg('--name')}))
elif a[:2]==['repository','clone']:print(json.dumps({'status':'cloned','parent':arg('--parent'),'path':arg('--parent')+'/'+arg('--name'),'name':arg('--name'),'request_id':arg('--request-id')}))
elif a[:1]==['worktrees']:print(json.dumps({'state':'not_repo','worktrees':[]}))
elif a[:2]==['project','ls']:print('[]')
)PY"));
    HgsClient client(FakeCli::program(script));SessionOrganization projects;const auto id=projects.createGroup("Work");projects.markImported("pc");projects.markImported("mac");
    ProjectsDialog view(&client,nullptr,false,&projects);FleetState fleet;BoxState pc;pc.host="pc";pc.ok=true;pc.metrics=QJsonObject{{"platform","linux"}};fleet.setLocal(pc,QDateTime::currentMSecsSinceEpoch());BoxState mac;mac.host="mac";mac.ok=true;fleet.setPeer(mac,QDateTime::currentMSecsSinceEpoch());view.setFleet(fleet);view.selectProject(id);view.show();
    QSignalSpy launched(&view,&ProjectsDialog::newSessionRequested);QSignalSpy paths(&view,&ProjectsDialog::newPathSessionRequested);
    QTimer::singleShot(0,&view,[&]{
        auto *choice=view.findChild<QDialog *>("cloneRepositoryComputerDialog");QVERIFY(choice);auto *computer=choice->findChild<QComboBox *>("cloneTargetComputer");computer->setCurrentIndex(computer->findData("mac"));
        QTimer::singleShot(0,&view,[&]{auto *form=view.findChild<CloneRepositoryDialog *>();QVERIFY(form);form->findChild<QLineEdit *>("cloneRepositoryUrl")->setText("https://example.invalid/team/project.git");form->findChild<QLineEdit *>("cloneParentFolder")->setText("/workspace");form->findChild<QPushButton *>("cloneRepositoryConfirm")->click();});
        choice->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Ok)->click();
    });
    QTimer::singleShot(10000,&view,[&]{for(auto *dialog:view.findChildren<QDialog *>())dialog->reject();});
    view.findChild<QPushButton *>("cloneProjectRepository")->click();
    QCOMPARE(projects.group(id)->folders.size(),1);QCOMPARE(projects.group(id)->folders.first().machine,"mac");QCOMPARE(projects.group(id)->folders.first().path,"/workspace/project");QCOMPARE(launched.size(),0);QCOMPARE(paths.size(),0);
}

void TestProjectsDialog::projectColorChangesRequireApply_data()
{
    QTest::addColumn<QString>("action");
    for(const auto &action:{"outside","escape","cancel","apply"})QTest::newRow(action)<<QString(action);
}
void TestProjectsDialog::projectColorChangesRequireApply()
{
    QFETCH(QString,action);
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-color-test");QCoreApplication::setApplicationName("projects");
    SessionOrganization projects;const auto id=projects.createGroup("Work");
    const auto original=projects.group(id)->color;const bool vivid=projects.group(id)->vivid;
    HgsClient client("/nonexistent/hgs");ProjectsDialog view(&client,nullptr,false,&projects);view.resize(900,650);view.selectProject(id);view.show();QVERIFY(QTest::qWaitForWindowExposed(&view));
    QSignalSpy changed(&view,&ProjectsDialog::organizationChanged);bool expired=false,handled=false;
    QTimer::singleShot(0,&view,[&]{
        auto *dialog=view.findChild<QDialog *>("projectColorDialog");QVERIFY(dialog);
        QTimer::singleShot(5000,dialog,[&,dialog]{expired=true;dialog->reject();});
        dialog->findChild<QLineEdit *>("projectColorHex")->setText("#e05999");
        dialog->findChild<QComboBox *>("projectColorStyle")->setCurrentIndex(vivid?0:1);
        QCOMPARE(projects.group(id)->color,original);QCOMPARE(changed.size(),0);
        auto *buttons=dialog->findChild<QDialogButtonBox *>();
        if(action=="outside"){
            const QPoint point(view.width()-8,view.height()-8);QVERIFY(!dialog->frameGeometry().contains(view.mapToGlobal(point)));
            QVERIFY(PopupMouse::click(&view,Qt::LeftButton,point));
        }else if(action=="escape")QTest::keyClick(dialog,Qt::Key_Escape);
        else buttons->button(action=="apply"?QDialogButtonBox::Apply:QDialogButtonBox::Cancel)->click();
        QTRY_VERIFY(!dialog->isVisible());handled=true;
    });
    view.findChild<QPushButton *>("projectColor")->click();QVERIFY(handled);QVERIFY(!expired);
    QCOMPARE(projects.group(id)->color,action=="apply"?QString("#e05999"):original);
    QCOMPARE(projects.group(id)->vivid,action=="apply"?!vivid:vivid);QCOMPARE(changed.size(),action=="apply"?1:0);
    QVERIFY(view.isVisible());
}

void TestProjectsDialog::customProjectColorRemainsStaged_data()
{
    QTest::addColumn<bool>("acceptPicker");QTest::addColumn<bool>("applyProject");
    QTest::newRow("outside-picker-then-apply")<<false<<true;
    QTest::newRow("accept-picker-then-cancel")<<true<<false;
    QTest::newRow("accept-picker-then-apply")<<true<<true;
}
void TestProjectsDialog::customProjectColorRemainsStaged()
{
    QFETCH(bool,acceptPicker);QFETCH(bool,applyProject);
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-color-test");QCoreApplication::setApplicationName("projects");
    SessionOrganization projects;const auto id=projects.createGroup("Work");const auto original=projects.group(id)->color;
    HgsClient client("/nonexistent/hgs");ProjectsDialog view(&client,nullptr,false,&projects);view.resize(900,650);view.selectProject(id);view.show();QVERIFY(QTest::qWaitForWindowExposed(&view));
    QSignalSpy changed(&view,&ProjectsDialog::organizationChanged);bool expired=false,handled=false;
    QTimer::singleShot(0,&view,[&]{
        auto *dialog=view.findChild<QDialog *>("projectColorDialog");QVERIFY(dialog);
        QTimer::singleShot(10000,dialog,[&,dialog]{expired=true;dialog->reject();});
        auto *hex=dialog->findChild<QLineEdit *>("projectColorHex");hex->setText("#2299aa");
        QTimer::singleShot(0,dialog,[&,dialog]{
            auto *picker=dialog->findChild<QColorDialog *>();QVERIFY(picker);
            QTimer::singleShot(5000,picker,[&,picker]{expired=true;picker->reject();});
            picker->setCurrentColor(QColor("#ee55aa"));
            if(acceptPicker)picker->accept();
            else{
                picker->move(dialog->frameGeometry().right()+15,dialog->frameGeometry().top());
                const QPoint point(8,8);QVERIFY(!picker->frameGeometry().contains(dialog->mapToGlobal(point)));
                QVERIFY(PopupMouse::click(dialog,Qt::LeftButton,point));
            }
            QTRY_VERIFY(!picker->isVisible());
        });
        dialog->findChild<QPushButton *>("projectCustomColor")->click();QVERIFY(!expired);QVERIFY(dialog->isVisible());
        QCOMPARE(hex->text(),acceptPicker?QString("#ee55aa"):QString("#2299aa"));
        QCOMPARE(projects.group(id)->color,original);QCOMPARE(changed.size(),0);
        dialog->findChild<QDialogButtonBox *>()->button(applyProject?QDialogButtonBox::Apply:QDialogButtonBox::Cancel)->click();handled=true;
    });
    view.findChild<QPushButton *>("projectColor")->click();QVERIFY(handled);QVERIFY(!expired);
    QCOMPARE(projects.group(id)->color,applyProject?(acceptPicker?QString("#ee55aa"):QString("#2299aa")):original);
    QCOMPARE(changed.size(),applyProject?1:0);
}

void TestProjectsDialog::worktreeOverviewIsVisibleWithoutSelectingAProjectFolder()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-worktree-test");QCoreApplication::setApplicationName("projects");
    SessionOrganization projects;HgsClient client("/nonexistent/hgs");ProjectsDialog dialog(&client,nullptr,false,&projects);
    dialog.resize(760,560);dialog.show();auto *button=dialog.findChild<QPushButton *>("projectWorktrees");QVERIFY(button);QVERIFY(button->isVisible());QVERIFY(button->isEnabled());
    bool opened=false;QTimer::singleShot(0,&dialog,[&]{auto *overview=dialog.findChild<QDialog *>("worktreesDialog");QVERIFY(overview);
        QVERIFY(overview->findChild<QLabel *>("worktreeOverviewStatus")->text().contains("Add a project folder"));opened=true;overview->reject();});
    button->click();QVERIFY(opened);
}

static QMenu *folderMenu(ProjectsDialog &dialog, const QString &id)
{
    auto *table = dialog.findChild<QTableWidget *>("projectFolders");
    for (int row = 0; row < table->rowCount(); ++row) if (table->item(row, 0)->data(Qt::UserRole).toString() == id) {
        table->customContextMenuRequested(table->visualItemRect(table->item(row, 0)).center());
        return dialog.findChild<QMenu *>("projectFolderMenu");
    }
    return nullptr;
}

static QAction *folderTarget(QMenu *menu, const QString &destination, bool copy)
{
    auto *targets = menu->findChild<QMenu *>(copy ? "copyProjectFolderTo" : "moveProjectFolderTo");
    if (targets) for (auto *action : targets->actions()) if (action->data().toString() == destination) return action;
    return nullptr;
}

void TestProjectsDialog::folderTransfersKeepMenuTargetsAndPersist_data()
{
    QTest::addColumn<bool>("copy"); QTest::addColumn<QString>("machine");
    QTest::newRow("move-local") << false << QString("local");
    QTest::newRow("copy-local") << true << QString("local");
    QTest::newRow("move-offline-remote") << false << QString("remote");
    QTest::newRow("copy-offline-remote") << true << QString("remote");
}

void TestProjectsDialog::folderTransfersKeepMenuTargetsAndPersist()
{
    QFETCH(bool, copy); QFETCH(QString, machine);
    QTemporaryDir directory; QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat); QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, directory.path());
    QCoreApplication::setOrganizationName("hgs-folder-transfer-test"); QCoreApplication::setApplicationName("projects");
#ifdef Q_OS_WIN
    // Project folders are POSIX paths on agent machines; a temporary folder of this
    // Windows computer cannot be one, so the files check below is Unix-only.
    const QString path = "/workspace/plain folder";
#else
    const auto path = directory.path() + "/plain folder"; QVERIFY(QDir().mkpath(path));
#endif
    SessionOrganization projects; const auto source = projects.createGroup("Source"), target = projects.createGroup("Work & docs");
    const auto original = projects.addFolder(source, machine, path, "Transfer this");
    const auto keep = projects.addFolder(source, machine, "/workspace/keep", "Keep this");
    projects.moveSession(projects.observe(machine, "agent/session", "run-1"), source);
    setJsonSetting("workspace/organization", QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    HgsClient client("/nonexistent/hgs"); ProjectsDialog dialog(&client); dialog.selectProject(source); dialog.show();
    auto *menu = folderMenu(dialog, original); QVERIFY(menu);
    QVERIFY(!folderTarget(menu, source, copy));
    auto *action = folderTarget(menu, target, copy); QVERIFY(action); QCOMPARE(action->text(), QString("Work && docs"));
    // A later selection and project navigation must not retarget the open menu.
    auto *table = dialog.findChild<QTableWidget *>("projectFolders");
    for (int row = 0; row < table->rowCount(); ++row)
        if (table->item(row, 0)->data(Qt::UserRole).toString() == keep) table->setCurrentCell(row, 0);
    dialog.selectProject(target);
    QSignalSpy changed(&dialog, &ProjectsDialog::organizationChanged); action->trigger(); QCOMPARE(changed.size(), 1);
    const SessionOrganization saved(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(saved.group(source)->folders.size(), copy ? 2 : 1); QCOMPARE(saved.group(target)->folders.size(), 1);
    const auto transferred = saved.group(target)->folders.first();
    QCOMPARE(transferred.machine, machine); QCOMPARE(transferred.path, path); QCOMPARE(transferred.name, QString("Transfer this"));
    if (copy) QVERIFY(transferred.id != original); else QCOMPARE(transferred.id, original);
    QCOMPARE(saved.groupFor(machine + "\nagent/session"), source);
#ifndef Q_OS_WIN
    QVERIFY(QDir(path).exists());
#endif
    QCOMPARE(table->rowCount(), 1); QCOMPARE(table->item(0, 0)->data(Qt::UserRole).toString(), transferred.id);
    delete menu;
}

void TestProjectsDialog::folderTransferMenusRejectStaleChanges_data()
{
    QTest::addColumn<QString>("change"); QTest::addColumn<bool>("copy");
    for (const auto &change : {"edited-folder", "removed-folder", "removed-source", "removed-destination", "hidden-destination"}) {
        QTest::newRow((QByteArray("move-") + change).constData()) << QString(change) << false;
        QTest::newRow((QByteArray("copy-") + change).constData()) << QString(change) << true;
    }
}

void TestProjectsDialog::folderTransferMenusRejectStaleChanges()
{
    QFETCH(QString, change); QFETCH(bool, copy);
    QTemporaryDir directory; QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat); QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, directory.path());
    QCoreApplication::setOrganizationName("hgs-folder-stale-test"); QCoreApplication::setApplicationName("projects");
    SessionOrganization projects; const auto source = projects.createGroup("Source"), target = projects.createGroup("Target");
    const auto original = projects.addFolder(source, "offline", "/workspace/folder", "Original");
    HgsClient client("/nonexistent/hgs"); ProjectsDialog dialog(&client, nullptr, false, &projects);
    dialog.selectProject(source); dialog.show(); auto *menu = folderMenu(dialog, original); QVERIFY(menu);
    auto *action = folderTarget(menu, target, copy); QVERIFY(action);
    if (change == "edited-folder") QVERIFY(projects.editFolder(source, original, "offline", "/workspace/changed", "Changed"));
    else if (change == "removed-folder") projects.removeFolder(source, original);
    else if (change == "removed-source") QVERIFY(projects.removeGroup(source));
    else if (change == "removed-destination") QVERIFY(projects.removeGroup(target));
    else {
        auto data = projects.toJson(); auto groups = data["projects"].toArray();
        for (int i = 0; i < groups.size(); ++i) {
            auto group = groups[i].toObject();
            if (group["id"] == target) { group["accessible"] = false; groups[i] = group; }
        }
        data["projects"] = groups; projects = SessionOrganization(data);
    }
    const auto before = projects.toJson(); QSignalSpy changed(&dialog, &ProjectsDialog::organizationChanged);
    action->trigger(); QCOMPARE(changed.size(), 0); QCOMPARE(projects.toJson(), before); delete menu;
}

void TestProjectsDialog::folderTransferMenusRespectVisibleProjects()
{
    QTemporaryDir directory; QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat); QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, directory.path());
    QCoreApplication::setOrganizationName("hgs-folder-targets-test"); QCoreApplication::setApplicationName("projects");
    QJsonObject source{{"id", "source"}, {"name", "Source"}, {"folders", QJsonArray{QJsonObject{
        {"id", "folder"}, {"machine", "offline"}, {"path", "/workspace/folder"}}}}};
    const QJsonObject hidden{{"id", "hidden"}, {"name", "Hidden"}, {"accessible", false}};
    SessionOrganization projects({{"default_project", "source"}, {"projects", QJsonArray{source, hidden}}});
    HgsClient client("/nonexistent/hgs"); ProjectsDialog dialog(&client, nullptr, false, &projects);
    dialog.selectProject("source"); dialog.show(); auto *menu = folderMenu(dialog, "folder"); QVERIFY(menu);
    QVERIFY(!menu->findChild<QMenu *>("moveProjectFolderTo")->isEnabled());
    QVERIFY(!menu->findChild<QMenu *>("copyProjectFolderTo")->isEnabled()); delete menu;
    dialog.findChild<QCheckBox *>("allSwarmProjects")->setChecked(true);
    menu = folderMenu(dialog, "folder"); QVERIFY(menu); QVERIFY(folderTarget(menu, "hidden", false));
    folderTarget(menu, "hidden", true)->trigger(); QCOMPARE(projects.group("source")->folders.size(), 1);
    QCOMPARE(projects.group("hidden")->folders.size(), 1);
    // Repeating Copy reuses the target and does not publish a redundant catalog edit.
    QSignalSpy changed(&dialog, &ProjectsDialog::organizationChanged);
    folderTarget(menu, "hidden", true)->trigger(); QCOMPARE(changed.size(), 0);
    folderTarget(menu, "hidden", false)->trigger(); QCOMPARE(changed.size(), 1);
    QVERIFY(projects.group("source")->folders.isEmpty()); QCOMPARE(projects.group("hidden")->folders.size(), 1);
    delete menu;
}

void TestProjectsDialog::sortingKeepsFolderIdentityAndLaunchTarget()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-folder-sort-test");QCoreApplication::setApplicationName("projects");
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;const auto project=projects.createGroup("Work");
    const auto zulu=projects.addFolder(project,"arch","/workspace/zulu","Zulu");
    const auto beta=projects.addFolder(project,"mac","/workspace/beta","Beta");
    projects.addFolder(project,"arch","/workspace/alpha");
    const auto otherBeta=projects.addFolder(project,"arch","/workspace/other","beta");
    projects.markImported("arch");projects.markImported("mac");const auto original=projects.toJson();
    // POSIX folders on every GUI: a Windows GUI would show its own machine's natively.
    BoxState local;local.host="arch";local.ok=true;local.metrics["platform"]="linux";BoxState remote;remote.host="mac";remote.ok=true;
    FleetState fleet;fleet.setLocal(local,QDateTime::currentMSecsSinceEpoch());fleet.setPeer(remote,QDateTime::currentMSecsSinceEpoch());
    ProjectsDialog dialog(&client,nullptr,false,&projects);dialog.setFleet(fleet);dialog.selectProject(project);
    auto *table=dialog.findChild<QTableWidget *>("projectFolders");QVERIFY(table);QCOMPARE(table->rowCount(),4);
    QCOMPARE(table->item(0,0)->text(),QString("alpha"));QCOMPARE(table->item(3,0)->text(),QString("Zulu"));
    for(int row=0;row<table->rowCount();++row) {
        const auto id=table->item(row,0)->data(Qt::UserRole).toString();
        const auto folders=projects.group(project)->folders;
        const auto folder=std::find_if(folders.cbegin(),folders.cend(),[&](const auto &f){return f.id==id;});
        QVERIFY(folder!=folders.cend());QCOMPARE(table->item(row,1)->text(),folder->machine);QCOMPARE(table->item(row,2)->text(),folder->path);
        if(id==beta)table->setCurrentCell(row,0);
    }
    table->sortItems(0,Qt::DescendingOrder);dialog.refresh();
    QCOMPARE(table->item(table->currentRow(),0)->data(Qt::UserRole).toString(),beta);
    QCOMPARE(table->item(0,0)->data(Qt::UserRole).toString(),zulu);
    QCOMPARE(projects.toJson(),original);
    QVERIFY(projects.editFolder(project,beta,"mac","/workspace/beta","aardvark"));dialog.refresh();
    QCOMPARE(table->item(table->currentRow(),0)->data(Qt::UserRole).toString(),beta);
    QSignalSpy launch(&dialog,&ProjectsDialog::newSessionRequested);
    dialog.findChild<QPushButton *>("newProjectFolderSession")->click();QCOMPARE(launch.size(),1);
    QCOMPARE(launch[0],QVariantList({project,QString("mac"),beta}));
    table->sortItems(1,Qt::AscendingOrder);dialog.refresh();
    QCOMPARE(table->horizontalHeader()->sortIndicatorSection(),1);
    QCOMPARE(table->item(table->currentRow(),0)->data(Qt::UserRole).toString(),beta);
    projects.removeFolder(project,otherBeta);dialog.refresh();QCOMPARE(table->rowCount(),3);
    QCOMPARE(table->item(table->currentRow(),0)->data(Qt::UserRole).toString(),beta);
}

void TestProjectsDialog::folderSplitPrioritizesListAndRemembersAdjustment()
{
    // #646: the folder table is as tall as its rows; the selected folder's
    // worktrees follow under its actions, and the handle between them sizes
    // the worktrees, remembered for the next time.
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-folder-split-test");QCoreApplication::setApplicationName("projects");clearSettings();
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;const auto project=projects.createGroup("Work");
    projects.addFolder(project,"arch","/workspace/repo");
    ProjectsDialog dialog(&client,nullptr,true,&projects);dialog.resize(1100,1000);dialog.selectProject(project);dialog.show();
    auto *table=dialog.findChild<QTableWidget *>("projectFolders");auto *split=dialog.findChild<QSplitter *>("projectFolderSplitter");QVERIFY(split);
    const int rows=table->horizontalHeader()->height()+table->verticalHeader()->defaultSectionSize()+8;
    QTRY_COMPARE(table->height(),rows);QVERIFY(!split->widget(1)->isVisible());
    table->setCurrentCell(0,0);QTRY_VERIFY(split->widget(1)->isVisible());QTest::qWait(30);
    QCOMPARE(table->height(),rows);
    const auto initial=split->sizes();QVERIFY(initial[1]>=200);
    // #700 (N3): a drag tracks the mouse from the press in many small moves,
    // as a hand does; the folder rows keep their height and nothing
    // compounds. A real mouse keeps its screen position while the handle
    // jumps back after each refit, so each step is a global point.
    auto *handle=split->handle(1);
    const auto drag=[&](int dy,int steps){
        const QPoint start=handle->mapToGlobal(handle->rect().center());
        QTest::mousePress(handle,Qt::LeftButton,Qt::NoModifier,handle->mapFromGlobal(start));
        for(int step=1;step<=steps;++step){
            QTest::mouseMove(handle,handle->mapFromGlobal(start+QPoint(0,dy*step/steps)));
            QTest::qWait(15);
        }
        QTest::mouseRelease(handle,Qt::LeftButton,Qt::NoModifier,handle->mapFromGlobal(start+QPoint(0,dy)));
        QTest::qWait(30);
    };
    drag(-60,12);   // up: the worktrees grow by the 60 px dragged, once
    QTRY_VERIFY2(qAbs(split->sizes()[1]-(initial[1]+60))<=6,qPrintable(QStringLiteral("%1 after %2 + 60").arg(split->sizes()[1]).arg(initial[1])));
    QTRY_COMPARE(table->height(),rows);QTRY_COMPARE(split->sizes()[0]+split->sizes()[1]+split->handleWidth(),split->height());
    QTRY_COMPARE(Settings::value("workspace/projectFolderWorktrees").toInt(),split->sizes()[1]);
    drag(400,20);   // down past the minimum: it stops at 120
    QTRY_COMPARE(split->sizes()[1],120);
    drag(-2000,20); // up past the page: at most two thirds of it
    QTRY_COMPARE(split->sizes()[1],qMax(240,dialog.height()*2/3));
    QTRY_COMPARE(Settings::value("workspace/projectFolderWorktrees").toInt(),qMax(240,dialog.height()*2/3));
    drag(split->sizes()[1]-(initial[1]-30),10);   // down to a middle height
    const int adjusted=split->sizes()[1];QVERIFY2(qAbs(adjusted-(initial[1]-30))<=6,qPrintable(QString::number(adjusted)));
    QTRY_COMPARE(Settings::value("workspace/projectFolderWorktrees").toInt(),adjusted);
    dialog.refresh();QTRY_COMPARE(split->sizes()[1],adjusted);
    table->clearSelection();QTRY_VERIFY(!split->widget(1)->isVisible());table->setCurrentCell(0,0);QTest::qWait(30);
    QVERIFY(qAbs(split->sizes()[1]-adjusted)<4);
    ProjectsDialog reopened(&client,nullptr,true,&projects);reopened.resize(dialog.size());reopened.selectProject(project);reopened.show();
    reopened.findChild<QTableWidget *>("projectFolders")->setCurrentCell(0,0);QTest::qWait(30);
    QVERIFY(qAbs(reopened.findChild<QSplitter *>("projectFolderSplitter")->sizes()[1]-adjusted)<4);
}

void TestProjectsDialog::folderDialogTakesDriveFoldersOnlyOnLocalWindows_data()
{
    QTest::addColumn<QString>("platform");
    QTest::newRow("windows") << QString("windows");
    QTest::newRow("linux") << QString("linux");
}

void TestProjectsDialog::folderDialogTakesDriveFoldersOnlyOnLocalWindows()
{
    QFETCH(QString, platform); const bool windows = platform == "windows";
    QTemporaryDir directory; QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat); QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, directory.path());
    QCoreApplication::setOrganizationName("hgs-folder-platform-test"); QCoreApplication::setApplicationName("projects");
    SessionOrganization projects; const auto project = projects.createGroup("Work");
    // A POSIX peer takes only POSIX folders, whatever this computer is.
    BoxState local; local.host = "desktop"; local.ok = true; local.metrics["platform"] = platform;
    BoxState peer; peer.host = "mac"; peer.ok = true; peer.metrics["platform"] = "macos";
    FleetState fleet; fleet.setLocal(local, QDateTime::currentMSecsSinceEpoch()); fleet.setPeer(peer, QDateTime::currentMSecsSinceEpoch());
    HgsClient client("/nonexistent/hgs"); ProjectsDialog dialog(&client, nullptr, false, &projects);
    dialog.setFleet(fleet); dialog.selectProject(project); dialog.show();
    const auto edit = [&](const std::function<void(QDialog *, QComboBox *, QLineEdit *, QPushButton *)> &check) {
        bool opened = false;
        QTimer::singleShot(0, &dialog, [&] {
            auto *folder = dialog.findChild<QDialog *>("projectFolderDialog"); QVERIFY(folder);
            check(folder, folder->findChild<QComboBox *>("projectFolderMachine"), folder->findChild<QLineEdit *>("projectFolderPath"),
                  folder->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Ok));
            opened = true; if (folder->isVisible()) folder->reject();
        });
        dialog.findChild<QPushButton *>("addProjectFolder")->click(); QVERIFY(opened);
    };
    edit([&](QDialog *folder, QComboBox *machine, QLineEdit *path, QPushButton *ok) {
        QCOMPARE(machine->currentData().toString(), QString());
        for (const char *drive : {"C:\\work", "c:/work/", "C:\\work/sub\\dir", "\\\\server\\share\\dir"}) {
            path->setText(drive); QVERIFY2(ok->isEnabled() == windows, drive);
        }
        for (const char *other : {"\\work", "C:work", "\\\\?\\C:\\work", "work"}) { path->setText(other); QVERIFY2(!ok->isEnabled(), other); }
        path->setText("/workspace/project"); QCOMPARE(ok->isEnabled(), !windows);
        machine->setCurrentIndex(machine->findData("mac")); QVERIFY(path->text().isEmpty());
        path->setText("C:\\work"); QVERIFY(!ok->isEnabled());
        path->setText("/Users/test/work"); QVERIFY(ok->isEnabled());
        machine->setCurrentIndex(machine->findData(QString()));
        path->setText(windows ? "C:\\Work\\Project\\" : "/workspace/project/"); QVERIFY(ok->isEnabled()); folder->accept();
    });
    const auto stored = windows ? QString("C:/Work/Project") : QString("/workspace/project");
    QCOMPARE(projects.group(project)->folders.size(), 1); QCOMPARE(projects.group(project)->folders[0].path, stored);
    QCOMPARE(projects.group(project)->folders[0].machine, QString("desktop"));
    // Windows folders show native separators; the stored form stays Qt's.
    auto *table = dialog.findChild<QTableWidget *>("projectFolders");
    QCOMPARE(table->item(0, 2)->text(), windows ? QString("C:\\Work\\Project") : stored);
    QCOMPARE(table->item(0, 0)->text(), QString(windows ? "Project" : "project"));
    // The same folder in another case or form is a duplicate only on Windows.
    edit([&](QDialog *, QComboBox *, QLineEdit *path, QPushButton *ok) {
        path->setText(windows ? "c:/work/project" : "/workspace/Project"); QCOMPARE(ok->isEnabled(), !windows);
        QCOMPARE(path->toolTip().isEmpty(), !windows);
        path->setText(windows ? "C:\\Work\\Project" : "/workspace/project"); QVERIFY(!ok->isEnabled());
    });
    // Editing shows the folder as Windows writes it.
    bool edited = false;
    QTimer::singleShot(0, &dialog, [&] {
        auto *folder = dialog.findChild<QDialog *>("projectFolderDialog"); QVERIFY(folder);
        QCOMPARE(folder->findChild<QLineEdit *>("projectFolderPath")->text(), windows ? QString("C:\\Work\\Project") : stored);
        edited = true; folder->reject();
    });
    table->setCurrentCell(0, 0); dialog.findChild<QPushButton *>("editProjectFolder")->click(); QVERIFY(edited);
    QCOMPARE(projects.group(project)->folders[0].path, stored);
}

void TestProjectsDialog::legacyAliasesOfALocalWindowsComputerBecomeFolders()
{
    QTemporaryDir directory; QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat); QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, directory.path());
    QCoreApplication::setOrganizationName("hgs-windows-import-test"); QCoreApplication::setApplicationName("projects");
    // `hgs project ls --json` of a Windows computer: drive paths as hgs.exe stores them.
    const auto hgs = directory.filePath("hgs");
    QVERIFY(FakeCli::install(hgs, R"PY(#!/usr/bin/env python3
import json
print(json.dumps([{"name": "DCB", "dir": "D:\\Work\\DCB", "src": "local", "exists": True},
                  {"name": "Tools", "dir": "D:\\Work\\Tools", "src": "local", "exists": True}]))
)PY"));
    SessionOrganization projects; const auto dcb = projects.createGroup("DCB");
    HgsClient client(FakeCli::program(hgs)); ProjectsDialog dialog(&client, nullptr, false, &projects);
    BoxState local; local.host = "desktop"; local.ok = true; local.localSessions = true; local.metrics["platform"] = "windows";
    FleetState fleet; fleet.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    dialog.setFleet(fleet);
    QTRY_VERIFY(projects.hasImported("desktop"));
    // The alias joins the project of its name; another one starts its own project.
    QCOMPARE(projects.group(dcb)->folders.size(), 1);
    QCOMPARE(projects.group(dcb)->folders.first().path, QString("D:/Work/DCB"));
    QString tools; for (const auto &group : projects.groups()) if (group.name == "Tools") tools = group.id;
    QVERIFY(!tools.isEmpty());
    QCOMPARE(projects.group(tools)->folders.first().path, QString("D:/Work/Tools"));
}

void TestProjectsDialog::legacyImportReportsAliasesItCannotAdd()
{
    QTemporaryDir directory; QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat); QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, directory.path());
    QCoreApplication::setOrganizationName("hgs-import-report-test"); QCoreApplication::setApplicationName("projects");
    // One alias this Windows computer takes, one it does not (a relative path).
    const auto hgs = directory.filePath("hgs");
    QVERIFY(FakeCli::install(hgs, R"PY(#!/usr/bin/env python3
import json
open(__file__ + ".calls", "a").write("call\n")
print(json.dumps([{"name": "DCB", "dir": "D:\\Work\\DCB", "src": "local", "exists": True},
                  {"name": "Loose", "dir": "work\\loose", "src": "local", "exists": False}]))
)PY"));
    SessionOrganization projects; const auto dcb = projects.createGroup("DCB");
    HgsClient client(FakeCli::program(hgs)); ProjectsDialog dialog(&client, nullptr, false, &projects);
    BoxState local; local.host = "desktop"; local.ok = true; local.localSessions = true; local.metrics["platform"] = "windows";
    FleetState fleet; fleet.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    dialog.setFleet(fleet);
    auto *status = dialog.findChild<QLabel *>("projectsStatus"); QVERIFY(status);
    QTRY_VERIFY(status->text().contains("Loose (work\\loose)"));
    QCOMPARE(projects.group(dcb)->folders.size(), 1);
    // The left-out alias starts no project, and the computer stays unimported so
    // that the next start tries again; until then no fleet update asks again.
    for (const auto &group : projects.groups()) QVERIFY(group.name != "Loose");
    QVERIFY(!projects.hasImported("desktop"));
    dialog.setFleet(fleet); QTest::qWait(300);
    QFile calls(hgs + ".calls"); QVERIFY(calls.open(QIODevice::ReadOnly));
    QCOMPARE(calls.readAll().count('\n'), 1);
}

void TestProjectsDialog::emptyListContextCreatesProject()
{
    QTemporaryDir directory;QVERIFY(directory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,directory.path());
    QCoreApplication::setOrganizationName("hgs-project-menu-test");QCoreApplication::setApplicationName("projects");
    HgsClient client("/nonexistent/hgs");SessionOrganization projects;
    ProjectsDialog dialog(&client,nullptr,false,&projects);dialog.show();QTest::qWait(30);
    auto *list=dialog.findChild<QListWidget*>("logicalProjects");QVERIFY(list);
    const auto selected=list->currentItem();const auto empty=list->viewport()->rect().bottomLeft()+QPoint(5,-5);QVERIFY(!list->itemAt(empty));
    list->customContextMenuRequested(empty);
    auto *menu=dialog.findChild<QMenu*>("projectListMenu");QVERIFY(menu);QCOMPARE(menu->actions().size(),1);QCOMPARE(list->currentItem(),selected);
    auto *add=menu->actions().first();QCOMPARE(add->objectName(),"addProjectAction");menu->hide();
    QTimer::singleShot(0,&dialog,[]{auto *input=qobject_cast<QInputDialog*>(QApplication::activeModalWidget());QVERIFY(input);input->setTextValue("Created from empty space");input->accept();});
    add->trigger();
    QVERIFY(std::any_of(projects.groups().cbegin(),projects.groups().cend(),[](const auto &p){return p.name=="Created from empty space";}));
    auto *item=list->currentItem();QVERIFY(item);list->customContextMenuRequested(list->visualItemRect(item).center());
    menu=qobject_cast<QMenu*>(QApplication::activePopupWidget());QVERIFY(menu);QCOMPARE(menu->actions().first()->objectName(),"addProjectAction");
    QVERIFY(menu->findChild<QAction*>("addProjectAction"));QCOMPARE(menu->actions().size(),5);
    QVERIFY(!menu->findChild<QAction*>("projectShowInGraph"));
    // #646: the menu renames the project in place.
    auto *rename=menu->findChild<QAction*>("renameProjectAction");QVERIFY(rename);
    auto *name=dialog.findChild<QLineEdit*>("projectName");QVERIFY(name->isHidden());
    rename->trigger();QVERIFY(!name->isHidden());QCOMPARE(name->text(),item->text());
    menu->close();
}

void TestProjectsDialog::acceptsPlainName()
{
    QString why;
    QVERIFY(ProjectsDialog::nameProblem(QStringLiteral("sample-project"), &why) == false);
    QVERIFY(why.isEmpty());
}

void TestProjectsDialog::rejectsSlashAndSpace()
{
    QString why;
    QVERIFY(!ProjectsDialog::nameProblem(QStringLiteral("a/b"), &why));
    QVERIFY(why.isEmpty());
    QVERIFY(!ProjectsDialog::nameProblem(QStringLiteral("a b"), &why));
}

void TestProjectsDialog::rejectsEmpty()
{
    QString why;
    QVERIFY(ProjectsDialog::nameProblem(QString(), &why));
}

void TestProjectsDialog::warnsOnDotOrColon()
{
    // Точка допустима (example.com — пример с точкой), но в имени сессии станет "_".
    QString why;
    QVERIFY(!ProjectsDialog::nameProblem(QStringLiteral("example.com"), &why));
    QVERIFY(ProjectsDialog::nameWarning(QStringLiteral("example.com")).isEmpty());
    QVERIFY(ProjectsDialog::nameWarning(QStringLiteral("plain")).isEmpty());
}

void TestProjectsDialog::nameDefaultsToBasename()
{
    QCOMPARE(ProjectsDialog::suggestName(QStringLiteral("/workspace/sample-tray")),
             QStringLiteral("sample-tray"));
    QCOMPARE(ProjectsDialog::suggestName(QStringLiteral("/workspace/sample-tray/")),
             QStringLiteral("sample-tray"));
    QCOMPARE(ProjectsDialog::suggestName(QStringLiteral("C:\\workspace\\sample-tray\\"), MachinePlatform::Style::Windows),
             QStringLiteral("sample-tray"));
    QCOMPARE(ProjectsDialog::suggestName(QStringLiteral("\\\\server\\share\\dir"), MachinePlatform::Style::Windows),
             QStringLiteral("dir"));
    QVERIFY(ProjectsDialog::suggestName(QStringLiteral("C:\\"), MachinePlatform::Style::Windows).isEmpty());
#ifndef Q_OS_WIN
    // A backslash is part of a POSIX name (a Windows build's QDir::cleanPath reads
    // it as a separator, as it did before machine styles).
    QCOMPARE(ProjectsDialog::suggestName(QStringLiteral("/workspace/a\\b")), QStringLiteral("a\\b"));
#endif
}

QTEST_MAIN(TestProjectsDialog)
#include "test_projectsdialog.moc"
