#include "Settings.h"
#include "SettingsJson.h"
#include "ProjectsDialog.h"
#include "CloneRepositoryDialog.h"
#include "WorktreePanel.h"
#include "WorktreesDialog.h"
#include "ProjectAppearance.h"
#include "IdentityBadge.h"
#include <QStyledItemDelegate>
#include <QGridLayout>
#include <QCheckBox>
#include "DirectoryDialog.h"
#include "WorkspaceIcons.h"
#include "WorkspaceList.h"
#include "WorkspaceColorDialog.h"
#include "CollapsibleBlock.h"
#include "IronRulesPanel.h"
#include "PageCards.h"
#include <QRegularExpression>
#include <QCollator>
#include <QApplication>
#include <QClipboard>
#include <QComboBox>
#include <QDesktopServices>
#include <QDialogButtonBox>
#include <QDir>
#include <QFormLayout>
#include <QFileInfo>
#include "PathProbe.h"
#include <QHeaderView>
#include <QInputDialog>
#include <QJsonDocument>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPushButton>
#include <QSettings>
#include <QSignalBlocker>
#include <QSplitter>
#include <QScrollArea>
#include <QScrollBar>
#include <QTableWidget>
#include <QVBoxLayout>
#include <QAction>
#include <QMenu>
#include <QPainter>
#include <QTimer>
#include <QUrl>
#include <QScopedValueRollback>
#include <numeric>

namespace {
QColor tok(bool dark,const char *name){return PageCards::token(dark,name);}
bool darkOf(const QStyleOptionViewItem &option){return option.widget&&option.widget->property("hgsDark").toBool();}
// A cell without Qt's own selection fill: the table paints its rows.
class PlainDelegate : public QStyledItemDelegate {
public:
    using QStyledItemDelegate::QStyledItemDelegate;
    void paint(QPainter *p,const QStyleOptionViewItem &option,const QModelIndex &index) const override {
        QStyleOptionViewItem plain(option);plain.state&=~(QStyle::State_Selected|QStyle::State_HasFocus|QStyle::State_MouseOver);
        QStyledItemDelegate::paint(p,plain,index);
    }
protected:
    // The cell's background only; the delegate paints what it shows.
    void background(QPainter *p,const QStyleOptionViewItem &option,const QModelIndex &index) const {
        QStyleOptionViewItem base(option);initStyleOption(&base,index);base.text.clear();base.icon=QIcon();
        base.state&=~(QStyle::State_Selected|QStyle::State_HasFocus|QStyle::State_MouseOver);
        option.widget->style()->drawControl(QStyle::CE_ItemViewItem,&base,p,option.widget);
    }
};
class FolderSplitterHandle : public QSplitterHandle {
public:
    using QSplitterHandle::QSplitterHandle;
protected:
    void paintEvent(QPaintEvent *) override {
        QPainter painter(this);painter.setPen(tok(parentWidget()->property("hgsDark").toBool(),"cardBorder"));
        const int y=rect().center().y();painter.drawLine(0,y,width()-1,y);
    }
};
class FolderSplitter : public QSplitter {
public:
    using QSplitter::QSplitter;
protected:
    QSplitterHandle *createHandle() override {return new FolderSplitterHandle(orientation(),this);}
};
class FolderSortItem : public QTableWidgetItem {
public:
    using QTableWidgetItem::QTableWidgetItem;
    bool operator<(const QTableWidgetItem &other) const override {
        QCollator collator;collator.setCaseSensitivity(Qt::CaseInsensitive);collator.setNumericMode(true);
        const int order=collator.compare(text(),other.text());
        return order ? order<0 : data(Qt::UserRole).toString()<other.data(Qt::UserRole).toString();
    }
};
// #646 (spec 7): the prototype's .ptbl rows - no grid; the hovered row fills
// with cardHover, the selected one with cardSel and its outline.
class PageTable : public QTableWidget {
public:
    explicit PageTable(QWidget *parent=nullptr):QTableWidget(parent){setMouseTracking(true);viewport()->setMouseTracking(true);}
protected:
    void paintEvent(QPaintEvent *event) override {
        {
            QPainter p(viewport());p.setRenderHint(QPainter::Antialiasing);
            const bool dark=property("hgsDark").toBool();
            const auto band=[&](int row,const QColor &fill,const QColor &edge){
                if(row<0||row>=rowCount())return;
                const QRectF r(.5,rowViewportPosition(row)+1.5,viewport()->width()-1,rowHeight(row)-3);
                p.setPen(edge.isValid()?QPen(edge,1):QPen(Qt::NoPen));p.setBrush(fill);p.drawRoundedRect(r,7,7);
            };
            const int selected=selectionModel()&&selectionModel()->hasSelection()?currentRow():-1;
            if(m_hover!=selected)band(m_hover,tok(dark,"cardHover"),{});
            band(selected,tok(dark,"cardSel"),tok(dark,"cardSelBorder"));
        }
        QTableWidget::paintEvent(event);
    }
    void mouseMoveEvent(QMouseEvent *event) override {
        const int row=rowAt(event->position().toPoint().y());if(row!=m_hover){m_hover=row;viewport()->update();}
        QTableWidget::mouseMoveEvent(event);
    }
    void leaveEvent(QEvent *event) override {m_hover=-1;viewport()->update();QTableWidget::leaveEvent(event);}
private:
    int m_hover=-1;
};
// A machine's label-colour tag (.mtag); a machine Zerus cannot reach is faded
// and named so in its tooltip.
class FolderMachineDelegate : public PlainDelegate {
public:
    using PlainDelegate::PlainDelegate;
    void paint(QPainter *p,const QStyleOptionViewItem &option,const QModelIndex &index) const override {
        background(p,option,index);
        const auto machine=index.data().toString();const bool dark=darkOf(option);
        const bool offline=index.data(Qt::UserRole+2).toBool();
        const int width=IdentityBadges::width(IdentityBadges::Machine,machine,option.font,option.rect.width()-16);
        const QRect tag(option.rect.left()+8,option.rect.center().y()-10,width,20);
        p->save();if(offline)p->setOpacity(.55);
        IdentityBadges::paint(p,tag,IdentityBadges::Machine,machine,dark);
        if(offline){p->setPen(QPen(tok(dark,"fg"),1));p->drawLine(tag.left()+5,tag.center().y()+1,tag.right()-5,tag.center().y()+1);}
        p->restore();
    }
};
// A folder's icon and name, with the "team" chip of a team member's worktree.
class FolderNameDelegate : public PlainDelegate {
public:
    using PlainDelegate::PlainDelegate;
    void paint(QPainter *p,const QStyleOptionViewItem &option,const QModelIndex &index) const override {
        background(p,option,index);
        const bool dark=darkOf(option);
        const QRect r=option.rect.adjusted(8,0,-6,0);
        int x=r.left();
        workspaceIcon("folder",tok(dark,"muted")).paint(p,QRect(x,r.center().y()-6,12,12));x+=22;
        const QString name=index.data().toString();const bool team=index.data(Qt::UserRole+1).toBool();
        QFont chipFont=option.font;chipFont.setPixelSize(11);
        const int chip=team?QFontMetrics(chipFont).horizontalAdvance(QObject::tr("team"))+14:0;
        p->setPen(tok(dark,"fg"));p->setFont(option.font);
        const int room=qMax(0,r.right()-x-(team?chip+6:0));
        const QString shown=QFontMetrics(option.font).elidedText(name,Qt::ElideRight,room);
        p->drawText(QRect(x,r.top(),room,r.height()),Qt::AlignVCenter,shown);
        if(team){
            const int left=x+QFontMetrics(option.font).horizontalAdvance(shown)+6;
            const QRect pill(left,r.center().y()-9,chip,19);
            QColor fill=tok(dark,"accent");fill.setAlphaF(dark?.10:.08);
            p->save();p->setRenderHint(QPainter::Antialiasing);p->setPen(Qt::NoPen);p->setBrush(fill);p->drawRoundedRect(pill,6,6);
            p->setFont(chipFont);p->setPen(tok(dark,"accent"));p->drawText(pill,Qt::AlignCenter,QObject::tr("team"));p->restore();
        }
    }
};
// A session's agent tile, its bold name and its team role (.pill).
class SessionNameDelegate : public PlainDelegate {
public:
    using PlainDelegate::PlainDelegate;
    void paint(QPainter *p,const QStyleOptionViewItem &option,const QModelIndex &index) const override {
        background(p,option,index);
        const bool dark=darkOf(option);const QRect r=option.rect.adjusted(8,0,-6,0);
        const QRect tile(r.left(),r.center().y()-8,17,17);
        p->save();p->setRenderHint(QPainter::Antialiasing);
        p->setPen(QPen(tok(dark,"cardBorder"),1));p->setBrush(tok(dark,"tile"));p->drawRoundedRect(QRectF(tile).adjusted(.5,.5,-.5,-.5),5,5);
        const auto agent=index.data(Qt::UserRole+3).toString();
        const QIcon logo=agentLogoIcon(agent,tok(dark,"fg"));
        if(!logo.isNull())logo.paint(p,tile.adjusted(3,3,-3,-3));
        p->restore();
        int x=tile.right()+10;
        QFont bold=option.font;bold.setWeight(QFont::DemiBold);
        QFont pillFont=option.font;pillFont.setPixelSize(11);
        const QString role=index.data(Qt::UserRole+2).toString();
        const int pill=role.isEmpty()?0:QFontMetrics(pillFont).horizontalAdvance(role)+14;
        const int room=qMax(0,r.right()-x-(pill?pill+6:0));
        const QString shown=QFontMetrics(bold).elidedText(index.data().toString(),Qt::ElideRight,room);
        p->setFont(bold);p->setPen(tok(dark,"fg"));p->drawText(QRect(x,r.top(),room,r.height()),Qt::AlignVCenter,shown);
        if(pill){
            const QRect chip(x+QFontMetrics(bold).horizontalAdvance(shown)+6,r.center().y()-9,pill,19);
            p->save();p->setRenderHint(QPainter::Antialiasing);p->setPen(Qt::NoPen);p->setBrush(tok(dark,"hover"));p->drawRoundedRect(chip,6,6);
            p->setFont(pillFont);p->setPen(tok(dark,"text3"));p->drawText(chip,Qt::AlignCenter,role);p->restore();
        }
    }
};
// A session's status as the prototype's .sbadge: Working in the accent,
// attention filled amber, Paused violet, Offline red, the rest quiet.
class SessionStatusDelegate : public PlainDelegate {
public:
    using PlainDelegate::PlainDelegate;
    void paint(QPainter *p,const QStyleOptionViewItem &option,const QModelIndex &index) const override {
        background(p,option,index);
        const QString text=index.data().toString();if(text.isEmpty())return;
        const bool dark=darkOf(option);const auto tone=index.data(Qt::UserRole+1).toString();
        QColor ink=tok(dark,"text3"),edge=tok(dark,"border"),fill(Qt::transparent);
        const auto mix=[](QColor color,double a){color.setAlphaF(a);return color;};
        if(tone=="active"){ink=tok(dark,"accent");edge=mix(ink,.55);}
        else if(tone=="warn"){fill=QColor(dark?"#ffda76":"#ffe7a6");edge=QColor(dark?"#ffda76":"#e8c463");ink=QColor("#3a2a00");}
        else if(tone=="paused"){ink=tok(dark,"violet");edge=mix(ink,.5);fill=mix(ink,.12);}
        else if(tone=="offline"){ink=tok(dark,"danger");edge=mix(ink,.5);}
        QFont font=option.font;font.setPixelSize(11);font.setWeight(QFont::DemiBold);
        const int w=QFontMetrics(font).horizontalAdvance(text)+16;
        const QRect pill(option.rect.right()-8-w,option.rect.center().y()-10,w,19);
        p->save();p->setRenderHint(QPainter::Antialiasing);p->setPen(QPen(edge,1));p->setBrush(fill);
        p->drawRoundedRect(QRectF(pill).adjusted(.5,.5,-.5,-.5),9.5,9.5);
        p->setFont(font);p->setPen(ink);p->drawText(pill,Qt::AlignCenter,text);p->restore();
    }
};
// #646 (spec 7): a project as the prototype's .li row - its folder icon in
// the project's colour, its name and how many sessions it has; the selected
// row is outlined. The default project carries a quiet "Default" tag.
class ProjectDelegate : public QStyledItemDelegate {
public:
    using QStyledItemDelegate::QStyledItemDelegate;
    QSize sizeHint(const QStyleOptionViewItem &,const QModelIndex &) const override {return {210,38};}
    void paint(QPainter *p,const QStyleOptionViewItem &option,const QModelIndex &index) const override {
        const bool dark=darkOf(option);
        const QRectF row=QRectF(option.rect).adjusted(.5,2.5,-.5,-1.5);
        p->save();p->setRenderHint(QPainter::Antialiasing);
        if(option.state&QStyle::State_Selected){p->setPen(QPen(tok(dark,"cardSelBorder"),1));p->setBrush(tok(dark,"cardSel"));p->drawRoundedRect(row,8,8);}
        else if(option.state&QStyle::State_MouseOver){p->setPen(Qt::NoPen);p->setBrush(tok(dark,"cardHover"));p->drawRoundedRect(row,8,8);}
        const QRect r=row.toRect().adjusted(9,0,-9,0);
        QColor color=index.data(Qt::UserRole+1).value<QColor>();if(!color.isValid())color=tok(dark,"folder");
        workspaceIcon("projects",color).paint(p,QRect(r.left(),r.center().y()-8,16,16));
        int right=r.right();
        QFont small=option.font;small.setPixelSize(11);small.setWeight(QFont::DemiBold);
        if(const int count=index.data(Qt::UserRole+4).toInt();count>=0){
            const QString text=QString::number(count);const int w=QFontMetrics(small).horizontalAdvance(text)+12;
            const QRect cnt(right-w+1,r.center().y()-9,w,18);
            p->setPen(Qt::NoPen);p->setBrush(tok(dark,"hover"));p->drawRoundedRect(cnt,6,6);
            p->setFont(small);p->setPen(tok(dark,"text3"));p->drawText(cnt,Qt::AlignCenter,text);right=cnt.left()-6;
        }
        if(index.data(Qt::UserRole+3).toBool()){
            QFont tagFont=option.font;tagFont.setPixelSize(11);
            const QString text=QObject::tr("Default");const int w=QFontMetrics(tagFont).horizontalAdvance(text)+14;
            const QRect tag(right-w+1,r.center().y()-9,w,19);
            p->setPen(Qt::NoPen);p->setBrush(tok(dark,"hover"));p->drawRoundedRect(tag,6,6);
            p->setFont(tagFont);p->setPen(tok(dark,"text3"));p->drawText(tag,Qt::AlignCenter,text);right=tag.left()-6;
        }
        QFont name=option.font;name.setPixelSize(13);
        const int left=r.left()+24;
        p->setFont(name);p->setPen(tok(dark,"fg"));
        p->drawText(QRect(left,r.top(),qMax(0,right-left),r.height()),Qt::AlignVCenter,QFontMetrics(name).elidedText(index.data().toString(),Qt::ElideRight,qMax(0,right-left)));
        p->restore();
    }
};
// A small button of the page (.btn.sm); ghost ones (.btn.ghost) have no box.
QPushButton *smallButton(const QString &text,const char *name,bool ghost=false)
{
    auto *button=new QPushButton(text);button->setObjectName(name);button->setAutoDefault(false);
    button->setProperty("pageSm",true);button->setProperty("pageGhost",ghost);button->setCursor(Qt::PointingHandCursor);
    return button;
}
}

ProjectsDialog::ProjectsDialog(HgsClient *client, QWidget *parent, bool embedded, SessionOrganization *projects)
    : QDialog(parent), m_client(client),
      m_ownedProjects(SettingsJson::object("workspace/organization")),
      m_projects(projects ? projects : &m_ownedProjects)
{
    // #744: a folder's state comes from PathProbe's worker; its answer updates
    // the selected folder's actions.
    connect(&PathProbe::instance(),&PathProbe::answered,this,[this](const QString &path){if(m_folders&&selectedFolder().path==path)updateFolderActions();});
    setObjectName("projectsPage"); setWindowTitle(tr("Projects")); resize(850, 560);
    if (embedded) setWindowFlags(Qt::Widget);
    auto *root = new QVBoxLayout(this); root->setContentsMargins(0, 0, 0, embedded ? 0 : 12); root->setSpacing(0);
    // #646 (spec 7): the prototype's page - the project list at the left
    // (Projects, + Project, compact rows, the hint), the project at the right.
    auto *split = new QSplitter; split->setObjectName("projectsSplit"); split->setChildrenCollapsible(false); split->setHandleWidth(1);
    root->addWidget(split, 1);
    auto *left = new QWidget; left->setObjectName("projectListColumn");
    auto *leftLayout = new QVBoxLayout(left); leftLayout->setContentsMargins(10,10,10,10); leftLayout->setSpacing(4);
    auto *heading = new QLabel(tr("Projects")); heading->setObjectName("projectsHeading");
    auto *headingRow=new QHBoxLayout;headingRow->setContentsMargins(4,4,4,8);headingRow->addWidget(heading);headingRow->addStretch();
    m_add=smallButton(tr("Project"),"newProject");m_add->setToolTip(tr("New project…"));headingRow->addWidget(m_add);
    leftLayout->addLayout(headingRow);
    auto *worktrees = smallButton(tr("Worktrees…"),"projectWorktrees",true);
    worktrees->setToolTip(tr("View worktrees across projects and computers, and review cleanup."));
    connect(worktrees, &QPushButton::clicked, this, [this] {
        WorktreesDialog dialog(m_client, *m_projects, m_fleet, m_allProjects->isChecked(), this);
        dialog.sessionRequested = [this](const QString &host, const QString &name) { emit relatedSessionRequested(host, name, {}); };
        m_worktreeOverview = &dialog;
        dialog.exec();
        m_worktreeOverview.clear();
        if (dialog.changed()) m_worktrees->refreshCatalog();
    });
    auto *swarm=smallButton(tr("Swarm…"),"projectSwarm",true);swarm->setToolTip(tr("Projects shared between your computers"));
    connect(swarm,&QPushButton::clicked,this,&ProjectsDialog::swarmRequested);
    m_list = new WorkspaceList; m_list->setObjectName("logicalProjects"); m_list->setItemDelegate(new ProjectDelegate(m_list));
    m_list->setMouseTracking(true);m_list->setFrameShape(QFrame::NoFrame);
    leftLayout->addWidget(m_list,100);
    m_list->setDragDropMode(QAbstractItemView::InternalMove);m_list->setDefaultDropAction(Qt::MoveAction);
    m_list->setDragDropOverwriteMode(false);m_list->setDropIndicatorShown(true);
    m_list->setAccessibleName(tr("Projects; drag to reorder, or use Alt+Up and Alt+Down"));
    m_moveUp=new QAction(tr("Move project up"),m_list);m_moveUp->setObjectName("moveProjectUp");
    m_moveDown=new QAction(tr("Move project down"),m_list);m_moveDown->setObjectName("moveProjectDown");
    m_moveUp->setShortcut(QKeySequence(Qt::ALT|Qt::Key_Up));m_moveDown->setShortcut(QKeySequence(Qt::ALT|Qt::Key_Down));
    for(auto *action:{m_moveUp,m_moveDown}){action->setShortcutContext(Qt::WidgetShortcut);m_list->addAction(action);}
    connect(m_moveUp,&QAction::triggered,this,[this]{moveProject(-1);});connect(m_moveDown,&QAction::triggered,this,[this]{moveProject(1);});
    m_list->setContextMenuPolicy(Qt::CustomContextMenu);
    connect(m_list,&QListWidget::customContextMenuRequested,this,[this](const QPoint &position){
        auto *item=m_list->itemAt(position);if(item)m_list->setCurrentItem(item);
        auto *menu=new QMenu(this);menu->setObjectName("projectListMenu");
        auto *add=menu->addAction(tr("New project…"),this,&ProjectsDialog::addProject);add->setObjectName("addProjectAction");
        if(item){
            auto *rename=menu->addAction(tr("Rename…"),this,&ProjectsDialog::startRename);rename->setObjectName("renameProjectAction");
            menu->addSeparator();menu->addAction(m_moveUp);menu->addAction(m_moveDown);
        }
        connect(menu,&QMenu::aboutToHide,menu,&QObject::deleteLater);menu->popup(m_list->viewport()->mapToGlobal(position));
    });
    connect(m_list->model(),&QAbstractItemModel::rowsMoved,this,[this]{
        QStringList order;for(int row=0;row<m_list->count();++row)order.append(m_list->item(row)->data(Qt::UserRole).toString());
        // Let Qt finish moving the row before rebuilding its items.
        QTimer::singleShot(0,this,[this,order]{
            QString before;for(auto it=order.crbegin();it!=order.crend();++it){m_projects->moveGroup(*it,before);before=*it;}
            changed();
        });
    });
    auto *hint = new QLabel(tr("Projects group sessions, folders on every machine and iron rules. Drag to reorder; the order syncs to your other computers."));
    hint->setWordWrap(true); hint->setObjectName("projectListHint"); hint->setContentsMargins(6,0,6,4); leftLayout->addWidget(hint);
    // Every swarm project, and the catalogs of computers not joined yet.
    m_allProjects=new QCheckBox(tr("All swarm projects"));m_allProjects->setObjectName("allSwarmProjects");
    m_allProjects->setToolTip(tr("Include projects on computers without a configured connection here."));
    m_allProjects->setChecked(Settings::value("workspace/allSwarmProjects",false).toBool());
    auto *catalogRow=new QHBoxLayout;catalogRow->setContentsMargins(6,4,6,0);catalogRow->addWidget(m_allProjects);catalogRow->addStretch();leftLayout->addLayout(catalogRow);
    m_swarmStatus=new QLabel; m_swarmStatus->setObjectName("projectSwarmStatus");m_swarmStatus->setTextFormat(Qt::PlainText);m_swarmStatus->setWordWrap(true);
    m_swarmStatus->setContentsMargins(6,0,6,0);leftLayout->addWidget(m_swarmStatus);
    connect(m_allProjects,&QCheckBox::toggled,this,[this](bool checked){Settings::set("workspace/allSwarmProjects",checked);refresh();});
    m_swarmConnectionPanel = new QWidget; m_swarmConnectionPanel->setObjectName("projectConnectionGuide");
    auto *connectionGuide = new QVBoxLayout(m_swarmConnectionPanel); connectionGuide->setContentsMargins(6,6,6,0); connectionGuide->setSpacing(6);
    m_swarmConnectionHint = new QLabel; m_swarmConnectionHint->setObjectName("projectConnectionHint");
    m_swarmConnectionHint->setTextFormat(Qt::PlainText); m_swarmConnectionHint->setWordWrap(true); connectionGuide->addWidget(m_swarmConnectionHint);
    m_connectSwarm = smallButton(tr("Connect project catalogs…"),"connectProjectCatalogs");
    connectionGuide->addWidget(m_connectSwarm,0,Qt::AlignLeft); leftLayout->addWidget(m_swarmConnectionPanel);
    connect(m_connectSwarm,&QPushButton::clicked,this,[this]{
        if (!m_unconnectedComputers.isEmpty()) emit connectSwarmRequested(m_unconnectedComputers.first());
    });
    m_swarmConnectionPanel->hide();
    leftLayout->addStretch(1);
    split->addWidget(left);
    auto *right = new QWidget; right->setObjectName("projectDetails");
    auto *form = new QVBoxLayout(right); form->setContentsMargins(22,14,22,26); form->setSpacing(10);
    // U25 (spec 7): the project as its icon and name, Color…, how many
    // sessions and teams it has, then Worktrees…, Swarm…, New team… and the
    // primary New session…. A click on the name renames the project in place.
    auto *header=new QHBoxLayout;header->setSpacing(8);
    m_headerIcon=new QLabel;m_headerIcon->setObjectName("projectHeaderIcon");header->addWidget(m_headerIcon);
    m_headerName=new QLabel;m_headerName->setObjectName("projectHeaderName");m_headerName->setTextFormat(Qt::PlainText);
    m_headerName->setFixedHeight(28);m_headerName->setCursor(Qt::IBeamCursor);m_headerName->setToolTip(tr("Click to rename"));m_headerName->installEventFilter(this);
    header->addWidget(m_headerName);
    m_name = new QLineEdit; m_name->setObjectName("projectName"); m_name->setMaxLength(80); m_name->setMinimumWidth(220); m_name->setFixedHeight(28); m_name->hide();
    m_name->setAccessibleName(tr("Project name"));m_name->installEventFilter(this);header->addWidget(m_name);
    m_color = smallButton(tr("Color…"),"projectColor",true); header->addWidget(m_color);
    m_summary=new QLabel;m_summary->setObjectName("projectSummary");m_summary->setTextFormat(Qt::PlainText);
    m_summary->setMinimumWidth(40);m_summary->setSizePolicy(QSizePolicy::Ignored,QSizePolicy::Preferred);header->addWidget(m_summary,1);
    header->addWidget(worktrees);header->addWidget(swarm);
    m_newTeam=smallButton(tr("New team…"),"projectNewTeam");header->addWidget(m_newTeam);
    connect(m_newTeam,&QPushButton::clicked,this,[this]{emit newTeamRequested(currentId());});
    m_newSession=smallButton(tr("New session…"),"newProjectFolderSession");
    PageCards::primary(m_newSession);header->addWidget(m_newSession);
    form->addLayout(header);
    m_id = new QLabel; m_id->setObjectName("projectId"); m_id->setTextFormat(Qt::PlainText); m_id->setTextInteractionFlags(Qt::TextSelectableByMouse);
    auto *idLabel = new QLabel(tr("ID")); idLabel->setObjectName("projectIdLabel"); idLabel->setProperty("pageMono",true);
    // The color preview lives in the project list; Color… in the header opens it.
    m_preview=new ProjectPreview(right); m_preview->setObjectName("projectColorPreview"); m_preview->hide();
    // The default checkbox, and the project ID at the right.
    auto *defaultRow=new QHBoxLayout;defaultRow->setSpacing(6);
    m_default=new QCheckBox(tr("Default for sessions started outside Zerus"));m_default->setObjectName("defaultProject");defaultRow->addWidget(m_default);defaultRow->addStretch();
    m_id->setProperty("pageMono",true);defaultRow->addWidget(idLabel);defaultRow->addWidget(m_id);form->addLayout(defaultRow);
    connect(m_default,&QCheckBox::clicked,this,[this](bool checked){if(checked){m_projects->setDefaultProject(currentId());changed();}else showProject();});
    m_folderSplitter=new FolderSplitter(Qt::Vertical);m_folderSplitter->setObjectName("projectFolderSplitter");
    m_folderSplitter->setChildrenCollapsible(false);m_folderSplitter->setHandleWidth(16);
    // U25 (spec 7): Folders, Sessions and Iron rules, each a collapsible block.
    m_foldersBlock=new CollapsibleBlock(QStringLiteral("projects/folders"),tr("Folders"));m_foldersBlock->setObjectName("projectFoldersBlock");
    m_foldersBlock->body()->addWidget(m_folderSplitter);
    // Each block sits in the prototype's card, as tall as what it shows.
    const auto inCard=[form](QWidget *block,const QString &name){
        QBoxLayout *box=nullptr;auto *card=PageCards::card(name,&box);box->setContentsMargins(12,10,12,10);box->setSpacing(8);
        box->addWidget(block);form->addWidget(card);
        return card;
    };
    inCard(m_foldersBlock,QStringLiteral("projectFoldersCard"));
    auto *folderPane=new QWidget;folderPane->setObjectName("projectFolderPane");
    auto *folderLayout=new QVBoxLayout(folderPane);folderLayout->setContentsMargins(0,0,0,0);folderLayout->setSpacing(8);
    m_folderSplitter->addWidget(folderPane);
    m_folders = new PageTable; m_folders->setObjectName("projectFolders"); m_folders->setColumnCount(4);
    m_folders->setHorizontalHeaderLabels({tr("Name"),tr("Computer"),tr("Folder"),tr("Kind")});
    m_folders->setSortingEnabled(true);m_folders->sortItems(0,Qt::AscendingOrder);
    m_folders->horizontalHeader()->setToolTip(tr("Click a column heading to sort folders."));
    m_folders->horizontalHeader()->setSectionResizeMode(0,QHeaderView::Interactive); m_folders->setColumnWidth(0,240);
    m_folders->horizontalHeader()->setSectionResizeMode(1,QHeaderView::Interactive); m_folders->setColumnWidth(1,114);
    m_folders->horizontalHeader()->setDefaultAlignment(Qt::AlignLeft|Qt::AlignVCenter);
    m_folders->horizontalHeader()->setSectionResizeMode(2,QHeaderView::Stretch);
    m_folders->horizontalHeader()->setSectionResizeMode(3,QHeaderView::Interactive); m_folders->setColumnWidth(3,220);
    m_folders->horizontalHeader()->setFixedHeight(24);m_folders->horizontalHeader()->setHighlightSections(false);
    m_folders->setItemDelegate(new PlainDelegate(m_folders));m_folders->setItemDelegateForColumn(0,new FolderNameDelegate(m_folders));
    m_folders->verticalHeader()->hide(); m_folders->verticalHeader()->setDefaultSectionSize(31); m_folders->setShowGrid(false);
    m_folders->setSelectionBehavior(QAbstractItemView::SelectRows); m_folders->setSelectionMode(QAbstractItemView::SingleSelection);
    m_folders->setWordWrap(false); m_folders->setTextElideMode(Qt::ElideMiddle);m_folders->setFrameShape(QFrame::NoFrame);
    // #700 (L3): Name and Kind shrink first; narrower still, the table scrolls sideways.
    m_folders->setHorizontalScrollBarPolicy(Qt::ScrollBarAsNeeded);m_folders->setFocusPolicy(Qt::StrongFocus);
    m_folders->installEventFilter(this);
    m_folders->setItemDelegateForColumn(1,new FolderMachineDelegate(m_folders));
    m_folders->setEditTriggers(QAbstractItemView::NoEditTriggers); folderLayout->addWidget(m_folders);
    auto *folderActions = new QHBoxLayout; folderActions->setSpacing(6);
    auto *addFolderButton = smallButton(tr("Folder"),"addProjectFolder",true);
    addFolderButton->setToolTip(tr("Add a folder to this project"));
    m_openFolder = smallButton(tr("Open folder"),"openProjectFolder");
    m_editFolder = smallButton(tr("Edit folder…"),"editProjectFolder");
    m_removeFolder = smallButton(tr("Remove folder"),"removeProjectFolder");
    m_removeFolder->setToolTip(tr("Remove this folder from the project. Files stay in place."));
    auto *clone=smallButton(tr("Clone repository…"),"cloneProjectRepository",true);
    connect(clone,&QPushButton::clicked,this,&ProjectsDialog::cloneRepository);
    m_addFolder=addFolderButton;
    m_foldersBlock->trailing()->addWidget(clone);m_foldersBlock->trailing()->addWidget(addFolderButton);folderActions->addWidget(m_editFolder); folderActions->addWidget(m_removeFolder); folderActions->addStretch(); folderActions->addWidget(m_openFolder); folderLayout->addLayout(folderActions);
    m_worktrees=new WorktreePanel(m_client,false,nullptr,WorktreePanel::Layout::Compact);m_folderSplitter->addWidget(m_worktrees);
    // The entire gap belongs to the handle, so its line sits between both action rows.
    m_worktrees->layout()->setContentsMargins(0,0,0,0);
    m_folderSplitter->setStretchFactor(0,1);m_folderSplitter->setStretchFactor(1,0);
    m_folderSplitter->handle(1)->setToolTip(tr("Drag to resize the folder's worktrees."));
    // #646: the folders keep their rows' height; dragging the handle sizes
    // the selected folder's worktrees, remembered for the next time.
    // #700 (N3): the handle's drag sets the worktrees' height from where it
    // was pressed: the mouse's distance from the press, counted once, in its
    // bounds (fitFolders moves the handle back after every refit, so Qt's
    // own splitter move would count the distance again). It also grows the
    // worktrees when the folder rows are already at their minimum. The
    // height is remembered when the handle is let go.
    m_folderSplitter->handle(1)->installEventFilter(this);
    m_worktrees->filterRequested=[this](const QString &path,bool checkout){const auto folder=selectedFolder();emit folderSessionsRequested(folder.machine==m_fleet.local().host?QString():folder.machine,path,checkout);};
    m_worktrees->sessionRequested=[this](const QString &name,const QString &archive){const auto folder=selectedFolder();emit relatedSessionRequested(folder.machine==m_fleet.local().host?QString():folder.machine,name,archive);};
    m_worktrees->launchRequested=[this](const QString &path){const auto folder=selectedFolder();if(folderLaunchProblem(folder).isEmpty())emit newPathSessionRequested(currentId(),folder.machine==m_fleet.local().host?QString():folder.machine,path);};
    // Sessions: one row each - agent, name and team role, machine, branch, status.
    m_sessionsBlock=new CollapsibleBlock(QStringLiteral("projects/sessions"),tr("Sessions"));m_sessionsBlock->setObjectName("projectSessionsBlock");
    m_sessions=new PageTable;m_sessions->setObjectName("projectSessions");m_sessions->setColumnCount(4);
    m_sessions->horizontalHeader()->hide();m_sessions->verticalHeader()->hide();m_sessions->verticalHeader()->setDefaultSectionSize(31);
    m_sessions->setShowGrid(false);m_sessions->setSelectionBehavior(QAbstractItemView::SelectRows);m_sessions->setSelectionMode(QAbstractItemView::SingleSelection);
    m_sessions->setEditTriggers(QAbstractItemView::NoEditTriggers);m_sessions->setWordWrap(false);m_sessions->setTextElideMode(Qt::ElideMiddle);
    m_sessions->setFrameShape(QFrame::NoFrame);m_sessions->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_sessions->horizontalHeader()->setSectionResizeMode(0,QHeaderView::Stretch);
    m_sessions->horizontalHeader()->setSectionResizeMode(1,QHeaderView::Fixed);m_sessions->setColumnWidth(1,124);
    m_sessions->horizontalHeader()->setSectionResizeMode(2,QHeaderView::Stretch);
    m_sessions->horizontalHeader()->setSectionResizeMode(3,QHeaderView::Fixed);m_sessions->setColumnWidth(3,130);
    m_sessions->setItemDelegate(new PlainDelegate(m_sessions));m_sessions->setItemDelegateForColumn(0,new SessionNameDelegate(m_sessions));
    m_sessions->setItemDelegateForColumn(1,new FolderMachineDelegate(m_sessions));m_sessions->setItemDelegateForColumn(3,new SessionStatusDelegate(m_sessions));
    m_sessions->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);m_sessions->setSizeAdjustPolicy(QAbstractScrollArea::AdjustToContents);
    connect(m_sessions,&QTableWidget::cellDoubleClicked,this,[this](int row,int){
        const auto *item=m_sessions->item(row,0);if(item)emit relatedSessionRequested(item->data(Qt::UserRole+1).toString(),item->data(Qt::UserRole).toString(),{});
    });
    m_sessionsBlock->body()->addWidget(m_sessions);
    // N23: the newest rows only; the rest are one click away in Sessions.
    m_allSessions=smallButton(QString(),"projectAllSessions",true);m_allSessions->hide();
    connect(m_allSessions,&QPushButton::clicked,this,[this]{emit projectSessionsRequested(currentId());});
    m_sessionsBlock->body()->addWidget(m_allSessions,0,Qt::AlignLeft);
    inCard(m_sessionsBlock,QStringLiteral("projectSessionsCard"));
    // Iron rules: the project's, as the session inspector edits them, with
    // their count and what they are for.
    m_rulesBlock=new CollapsibleBlock(QStringLiteral("projects/ironRules"),tr("Iron rules"));m_rulesBlock->setObjectName("projectRulesBlock");
    auto *rulesHint=new QLabel(tr("Sent at the start of every message to this project’s agents."));rulesHint->setObjectName("projectRulesHint");
    // Right after the count, as the prototype has it.
    if(auto *head=qobject_cast<QBoxLayout *>(m_rulesBlock->trailing()->parent()))head->insertWidget(1,rulesHint);
    else m_rulesBlock->trailing()->addWidget(rulesHint);
    m_rules=new IronRulesPanel;m_rules->setObjectName("projectIronRules");m_rulesBlock->body()->addWidget(m_rules);inCard(m_rulesBlock,QStringLiteral("projectRulesCard"));
    // The block's header names them and counts them.
    for(const char *name:{"inspectorPanelTitle","ironRulesCount"})if(auto *own=m_rules->findChild<QLabel *>(name))own->hide();
    m_rules->changed=[this](int count){m_rulesBlock->setNote(QString::number(count));};
    auto *foot = new QHBoxLayout; m_remove = smallButton(tr("Delete project…"),"deleteProject"); PageCards::danger(m_remove); foot->addWidget(m_remove); foot->addStretch(); form->addLayout(foot);
    // #700 (N4): the page's message stays in view under the details, outside their scroll.
    m_status = new QLabel; m_status->setObjectName("projectsStatus"); m_status->setTextFormat(Qt::PlainText); m_status->setWordWrap(true);
    m_status->setContentsMargins(22,6,22,8); m_status->hide();
    // The room left over goes under the blocks.
    form->addStretch(1);
    // U25: a short window scrolls the project instead of squeezing its blocks.
    auto *rightScroll=new QScrollArea;rightScroll->setObjectName("projectDetailsScroll");rightScroll->setWidgetResizable(true);
    rightScroll->setFrameShape(QFrame::NoFrame);rightScroll->setWidget(right);rightScroll->setAutoFillBackground(false);
    rightScroll->viewport()->setAutoFillBackground(false);right->setAutoFillBackground(false);
    auto *rightBox=new QWidget;rightBox->setObjectName("projectDetailsColumn");
    auto *rightColumn=new QVBoxLayout(rightBox);rightColumn->setContentsMargins(0,0,0,0);rightColumn->setSpacing(0);
    rightColumn->addWidget(rightScroll,1);rightColumn->addWidget(m_status);
    split->addWidget(rightBox); split->setStretchFactor(0,0); split->setStretchFactor(1,1); split->setSizes({300,900});
    if (!embedded) { auto *close = new QDialogButtonBox(QDialogButtonBox::Close); close->setContentsMargins(22,0,22,0); connect(close,&QDialogButtonBox::rejected,this,&QDialog::accept); root->addWidget(close); }
    connect(m_add,&QPushButton::clicked,this,&ProjectsDialog::addProject);
    connect(m_remove,&QPushButton::clicked,this,&ProjectsDialog::removeProject);
    connect(addFolderButton,&QPushButton::clicked,this,&ProjectsDialog::addFolder);
    connect(m_openFolder,&QPushButton::clicked,this,[this]{openFolder(selectedFolder());});
    connect(m_newSession,&QPushButton::clicked,this,[this]{newFolderSession(currentId(),selectedFolder());});
    connect(m_removeFolder,&QPushButton::clicked,this,[this]{
        // U25 (spec 7): a team member's own worktree or clone goes with its team.
        bool team=false;folderKind(selectedFolder(),&team);
        if(team){setStatus(tr("Team folders are removed with the team"),true);return;}
        removeFolder(currentId(),selectedFolder().id);
    });
    connect(m_editFolder,&QPushButton::clicked,this,[this]{const int row=m_folders->currentRow();if(row>=0)editFolder(m_folders->item(row,0)->data(Qt::UserRole).toString());});
    connect(m_folders,&QTableWidget::cellDoubleClicked,this,[this](int row,int){editFolder(m_folders->item(row,0)->data(Qt::UserRole).toString());});
    connect(m_list,&QListWidget::currentRowChanged,this,[this]{if(!m_name->isHidden())finishRename(false);showProject();});
    connect(m_folders,&QTableWidget::itemSelectionChanged,this,&ProjectsDialog::updateFolderActions);
    m_folders->setContextMenuPolicy(Qt::CustomContextMenu);
    connect(m_folders,&QTableWidget::customContextMenuRequested,this,&ProjectsDialog::showFolderMenu);
    connect(m_name,&QLineEdit::textEdited,this,[this]{emit catalogDraftEditing(true);});
    connect(m_name,&QLineEdit::editingFinished,this,[this]{finishRename(true);});
    connect(m_color,&QPushButton::clicked,this,&ProjectsDialog::chooseColor);
    m_worktreeHeight=Settings::value("workspace/projectFolderWorktrees",0).toInt();
    if(m_worktreeHeight>0)m_worktreeHeight=boundedWorktrees(m_worktreeHeight);
    m_live=new QTimer(this);m_live->setSingleShot(true);m_live->setInterval(1000);
    connect(m_live,&QTimer::timeout,this,&ProjectsDialog::refreshLive);
    connect(m_client,&HgsClient::projectsForHostReady,this,&ProjectsDialog::importLegacy);
    connect(m_client,&HgsClient::projectsFailed,this,[this](const QString &host,const QString &){m_importing.remove(host);});
    refresh();
}
bool ProjectsDialog::eventFilter(QObject *watched, QEvent *event)
{
    if(m_folderSplitter&&m_folderSplitter->count()>1&&watched==m_folderSplitter->handle(1)){
        const auto *mouse=static_cast<QMouseEvent *>(event);
        if(event->type()==QEvent::MouseButtonPress&&mouse->button()==Qt::LeftButton&&m_worktrees->isVisible()){
            m_dragging=true;m_dragFrom=m_folderSplitter->sizes().value(1);m_dragY=qRound(mouse->globalPosition().y());return true;
        }
        if(event->type()==QEvent::MouseMove&&m_dragging){
            const int height=boundedWorktrees(m_dragFrom-(qRound(mouse->globalPosition().y())-m_dragY));
            if(height!=m_worktreeHeight){m_worktreeHeight=height;fitFolders();}
            return true;
        }
        if(event->type()==QEvent::MouseButtonRelease&&m_dragging){
            m_dragging=false;Settings::set("workspace/projectFolderWorktrees",m_worktreeHeight);return true;
        }
    }
    if(m_folders&&watched==m_folders&&event->type()==QEvent::Resize)QTimer::singleShot(0,this,[this]{fitFolderColumns();});
    if(watched==m_headerName&&event->type()==QEvent::MouseButtonRelease&&static_cast<QMouseEvent *>(event)->button()==Qt::LeftButton){startRename();return true;}
    if(watched==m_name&&event->type()==QEvent::KeyPress){
        // Both keys stop here: QLineEdit passes Enter on, and the dialog
        // would press whatever button takes the focus from the hidden field.
        const int key=static_cast<QKeyEvent *>(event)->key();
        if(key==Qt::Key_Escape){finishRename(false);return true;}
        if(key==Qt::Key_Return||key==Qt::Key_Enter){finishRename(true);return true;}
    }
    return QDialog::eventFilter(watched,event);
}
int ProjectsDialog::boundedWorktrees(int height) const
{
    // At least a few rows; at most two thirds of the page.
    return qBound(120,height,qMax(240,this->height()*2/3));
}
void ProjectsDialog::fitFolderColumns()
{
    // Computer keeps its width; Name and Kind give up room first, down to
    // 110 px each, while Folder stretches. Narrower still, Folder keeps 180 px
    // and the table scrolls sideways to Kind.
    const int available=m_folders->viewport()->width();
    if(available<=0)return;
    const int computer=m_folders->columnWidth(1),folder=180,name=240,kind=220,least=110;
    const int spare=available-computer-folder;
    const double share=spare>=name+kind?1.0:std::max(0.0,double(spare)/double(name+kind));
    const int nameWidth=std::max(least,int(name*share)),kindWidth=std::max(least,int(kind*share));
    auto *header=m_folders->horizontalHeader();
    const bool scroll=spare<2*least;
    const auto mode=scroll?QHeaderView::Interactive:QHeaderView::Stretch;
    if(header->sectionResizeMode(2)!=mode){header->setSectionResizeMode(2,mode);if(scroll)m_folders->setColumnWidth(2,folder);}
    if(m_folders->columnWidth(0)!=nameWidth)m_folders->setColumnWidth(0,nameWidth);
    if(m_folders->columnWidth(3)!=kindWidth)m_folders->setColumnWidth(3,kindWidth);
    QTimer::singleShot(0,this,[this]{fitFolders();});
}
void ProjectsDialog::setStatus(const QString &text, bool error)
{
    // #700 (L1, N4): every message says whether it is a refusal; an empty
    // one hides the line.
    m_status->setText(text);m_status->setVisible(!text.isEmpty());
    if(m_status->property("pageError").toBool()!=error){m_status->setProperty("pageError",error);m_status->style()->unpolish(m_status);m_status->style()->polish(m_status);}
}
void ProjectsDialog::startRename()
{
    const auto *project=m_projects->group(currentId());if(!project)return;
    // #700 (L2): the rename belongs to this project, whatever the list selects later.
    m_renameId=project->id;
    m_name->setText(project->name);m_headerName->hide();m_name->show();m_name->setFocus(Qt::OtherFocusReason);m_name->selectAll();
}
void ProjectsDialog::finishRename(bool save)
{
    if(m_renaming)return;
    const QScopedValueRollback<bool> once(m_renaming,true);
    const bool editing=!m_name->isHidden();
    const auto name=m_name->text().trimmed();
    bool renamed=false;
    // The project the rename started on, or the selected one when the field
    // is set without opening it; a project that is gone takes no name.
    const QString target=editing&&!m_renameId.isEmpty()?m_renameId:currentId();
    if(save&&!name.isEmpty()&&m_projects->group(target)&&name!=m_projects->group(target)->name){m_projects->renameGroup(target,m_name->text());changed();renamed=true;}
    m_renameId.clear();
    if(editing){m_name->hide();m_headerName->show();}
    if(save||editing)emit catalogDraftEditing(false);
    if(!renamed&&(editing||name.isEmpty()))showProject();
}
QString ProjectsDialog::currentId() const { return m_list->currentItem() ? m_list->currentItem()->data(Qt::UserRole).toString() : QString(); }
void ProjectsDialog::setSwarmStatus(const QString &text,bool error)
{
    m_swarmStatus->setText(text);m_swarmStatus->setToolTip(text);
    m_swarmStatus->setStyleSheet(error?QStringLiteral("color: #db9f42;"):QString());
}
void ProjectsDialog::setSwarmSnapshot(const QJsonObject &snapshot)
{
    m_swarmSnapshot = snapshot; updateSwarmConnections();
}
void ProjectsDialog::updateSwarmConnections()
{
    m_unconnectedComputers.clear();
    const auto peers = m_swarmSnapshot.value("peers").toObject();
    // A configured SSH connection permits session access; it does not join
    // independent project catalogs. Only the local swarm snapshot proves that.
    for (const auto &computer : m_fleet.peerNames())
        if (!peers.contains(computer)) m_unconnectedComputers.append(computer);
    const bool known = !m_swarmSnapshot.value("node_id").toString().isEmpty();
    m_swarmConnectionPanel->setVisible(known && !m_unconnectedComputers.isEmpty());
    m_connectSwarm->setEnabled(known && !m_unconnectedComputers.isEmpty());
    m_swarmConnectionHint->setText(tr("Session connections for %1 do not share project catalogs yet. Preview the other catalog, match projects, then explicitly join to share their grouping.").arg(m_unconnectedComputers.join(", ")));
}
void ProjectsDialog::refresh()
{
    if (m_worktreeOverview) m_worktreeOverview->setContext(*m_projects, m_fleet, m_allProjects->isChecked());
    // #700 (L2): a rename whose project is gone is cancelled.
    if (!m_name->isHidden() && !m_renameId.isEmpty() && !m_projects->group(m_renameId)) finishRename(false);
    const auto selected = currentId(); const QSignalBlocker block(m_list); m_list->clear();
    const auto counts = m_countsProvider ? m_countsProvider() : QHash<QString, int>();
    for (const auto &project : m_projects->groups()) {
        if(!project.accessible&&!m_allProjects->isChecked())continue;
        auto *item = new QListWidgetItem(workspaceIcon("projects",QColor(project.color)),project.name,m_list); item->setData(Qt::UserRole,project.id); item->setData(Qt::UserRole+1,QColor(project.color)); item->setData(Qt::UserRole+2,project.vivid);
        const bool isDefault=project.id==m_projects->defaultProject();item->setData(Qt::UserRole+3,isDefault);
        item->setData(Qt::UserRole+4,m_countsProvider?counts.value(project.id):-1);
        item->setFlags((item->flags()|Qt::ItemIsDragEnabled)&~Qt::ItemIsDropEnabled);
        const auto description=isDefault?tr("%1, default project for sessions started outside Zerus").arg(project.name):project.name;
        item->setData(Qt::AccessibleTextRole,description);item->setToolTip(description+tr("\nDrag to reorder, or use Alt+Up / Alt+Down."));
        if (project.id == selected) m_list->setCurrentItem(item);
    }
    if (!m_list->currentItem() && m_list->count()) m_list->setCurrentRow(0);
    // The rows keep their height; the hint follows right under them.
    m_list->setMaximumHeight(qMax(40, m_list->count() * 38 + 10));
    showProject();
}
void ProjectsDialog::moveProject(int offset)
{
    const auto groups=m_projects->groups();const auto id=currentId();int index=0;
    while(index<groups.size()&&groups[index].id!=id)++index;
    if(index>=groups.size()||index+offset<0||index+offset>=groups.size())return;
    const auto before=offset<0?groups[index-1].id:index+2<groups.size()?groups[index+2].id:QString();
    m_projects->moveGroup(id,before);changed();
}
void ProjectsDialog::showProject()
{
    m_moveUp->setEnabled(m_list->currentRow()>0);m_moveDown->setEnabled(m_list->currentRow()>=0&&m_list->currentRow()+1<m_list->count());
    const auto *project = m_projects->group(currentId()); if (!project) return;
    if (m_name->isHidden()) m_name->setText(project->name);
    m_name->setEnabled(true); m_id->setText(project->id);
    m_headerName->setText(project->name);
    m_headerIcon->setPixmap(workspaceIcon("projects",QColor(project->color).isValid()?QColor(project->color):PageCards::token(m_dark,"folder")).pixmap(20,20));
    m_rules->setProject(project->id,project->name);m_rulesBlock->setNote(QString::number(m_rules->rules().size()));
    m_projectSessions=m_sessionsProvider?m_sessionsProvider(project->id):QList<ProjectSession>{};
    showSessions();
    m_preview->setProject(project->name,QColor(project->color),project->vivid,project->id==m_projects->defaultProject());
    m_default->setChecked(project->id==m_projects->defaultProject());
    const auto selected=selectedFolder().id;const QSignalBlocker blockFolders(m_folders);
    m_remove->setEnabled(m_projects->groups().size()>1);m_folders->setSortingEnabled(false); m_folders->setRowCount(0);
    const auto kinds=folderKinds();
    QFont mono=m_folders->font();mono.setFamilies({QStringLiteral("Cascadia Mono"),QStringLiteral("Consolas"),mono.family()});mono.setPixelSize(12);mono.setWeight(QFont::DemiBold);
    for (const auto &folder : project->folders) {
        const int row = m_folders->rowCount(); m_folders->insertRow(row);
        const auto style = folderStyle(folder.machine); const auto path = MachinePlatform::native(folder.path, style);
        const QString name = folder.name.isEmpty() ? suggestName(folder.path, style) : folder.name;
        m_folders->setItem(row,0,new FolderSortItem(name)); m_folders->item(row,0)->setData(Qt::UserRole,folder.id);
        m_folders->item(row,0)->setToolTip(name);
        m_folders->setItem(row,1,new FolderSortItem(folder.machineName.isEmpty()?folder.machine:folder.machineName)); m_folders->setItem(row,2,new FolderSortItem(path));
        const auto kind=kinds.value(folder.id);
        m_folders->item(row,0)->setData(Qt::UserRole+1,kind.team);
        auto *kindItem=new FolderSortItem(kind.text);kindItem->setForeground(PageCards::token(m_dark,"dim"));kindItem->setToolTip(kind.text);m_folders->setItem(row,3,kindItem);
        for(int column=1;column<4;++column)m_folders->item(row,column)->setData(Qt::UserRole,folder.id);
        m_folders->item(row,2)->setToolTip(path);m_folders->item(row,2)->setFont(mono);
        // #646: a machine Zerus cannot reach shows faded.
        const bool local=folder.machine==m_fleet.local().host;const auto *box=local?&m_fleet.local():m_fleet.peer(folder.machine);
        const bool offline=box&&!box->ok;m_folders->item(row,1)->setData(Qt::UserRole+2,offline);
        m_folders->item(row,1)->setToolTip(offline?tr("%1 is offline").arg(m_folders->item(row,1)->text()):m_folders->item(row,1)->text());
    }
    m_foldersBlock->setNote(QString::number(project->folders.size()));
    m_folders->setSortingEnabled(true);
    for(int row=0;row<m_folders->rowCount();++row)if(m_folders->item(row,0)->data(Qt::UserRole).toString()==selected){m_folders->setCurrentCell(row,0);break;}
    updateFolderActions();
}
void ProjectsDialog::fitFolders()
{
    // #646: the table is as tall as its rows (twelve, then it scrolls), never
    // the empty height of the page; the selected folder's worktrees follow.
    const int rowHeight=m_folders->verticalHeader()->defaultSectionSize(),header=m_folders->horizontalHeader()->height();
    // #700 (L3): a sideways scrollbar adds its height, so it covers no row.
    const int bar=m_folders->horizontalScrollBar()->isVisible()?m_folders->horizontalScrollBar()->sizeHint().height():0;
    const int table=header+qMax(1,qMin(m_folders->rowCount(),12))*rowHeight+8+bar;   // room under the rows: a right-click there adds a folder
    m_folders->setMinimumHeight(header+rowHeight+8+bar);m_folders->setMaximumHeight(table);
    auto *pane=m_folderSplitter->widget(0);
    const int paneHeight=table+pane->layout()->spacing()+m_editFolder->sizeHint().height();
    if(m_worktrees->isHidden()){m_folderSplitter->setFixedHeight(paneHeight);return;}
    const int worktrees=m_worktreeHeight>0?m_worktreeHeight:qBound(200,m_worktrees->sizeHint().height(),240);
    m_folderSplitter->setFixedHeight(paneHeight+m_folderSplitter->handleWidth()+worktrees);
    m_folderSplitter->setSizes({paneHeight,worktrees});
}
void ProjectsDialog::showSessions()
{
    // Sessions, how many of them are in teams, and in how many teams.
    const auto &sessions=m_projectSessions;
    int inTeams=0;QSet<QString> teams;
    for(const auto &session:sessions)if(!session.team.isEmpty()){++inTeams;teams.insert(session.team);}
    const QString sessionCount=sessions.size()==1?tr("1 session"):tr("%1 sessions").arg(sessions.size());
    const QString summary=tr("%1 · %2 in teams · %3").arg(sessionCount).arg(inTeams).arg(teams.size()==1?tr("1 team"):tr("%1 teams").arg(teams.size()));
    m_summary->setText(summary);m_sessionsBlock->setNote(summary);
    // The newest rows (N23: a project can hold thousands of sessions).
    QList<int> order(sessions.size());std::iota(order.begin(),order.end(),0);
    std::stable_sort(order.begin(),order.end(),[&sessions](int a,int b){return sessions[a].created>sessions[b].created;});
    if(order.size()>kSessionRows)order.resize(kSessionRows);
    // A refresh keeps the selected session selected.
    QString selected;
    if(const auto *current=m_sessions->currentRow()>=0?m_sessions->item(m_sessions->currentRow(),0):nullptr)
        selected=current->data(Qt::UserRole+1).toString()+'\n'+current->data(Qt::UserRole).toString();
    const QSignalBlocker block(m_sessions);
    m_sessions->setRowCount(order.size());
    QFont bold=m_sessions->font();bold.setWeight(QFont::DemiBold);
    QFont mono=m_sessions->font();mono.setFamilies({QStringLiteral("Cascadia Mono"),QStringLiteral("Consolas"),mono.family()});mono.setPixelSize(12);
    int current=-1;
    for(int row=0;row<order.size();++row){
        const auto &session=sessions[order[row]];
        auto *name=new QTableWidgetItem(session.title);
        name->setFont(bold);name->setData(Qt::UserRole,session.name);name->setData(Qt::UserRole+1,session.host);name->setToolTip(session.name);
        name->setData(Qt::UserRole+2,session.role);name->setData(Qt::UserRole+3,session.agent);
        m_sessions->setItem(row,0,name);
        auto *machine=new QTableWidgetItem(session.machine);machine->setData(Qt::UserRole+2,session.tone=="offline");m_sessions->setItem(row,1,machine);
        auto *branch=new QTableWidgetItem(session.branch);branch->setForeground(PageCards::token(m_dark,"dim"));branch->setFont(mono);
        m_sessions->setItem(row,2,branch);
        auto *status=new QTableWidgetItem(session.status);status->setData(Qt::UserRole+1,session.tone);m_sessions->setItem(row,3,status);
        if(session.host+'\n'+session.name==selected)current=row;
    }
    if(current>=0)m_sessions->setCurrentCell(current,0);
    m_sessions->setFixedHeight(qMax(31,m_sessions->rowCount()*m_sessions->verticalHeader()->defaultSectionSize()+2));
    m_allSessions->setText(tr("Show all %1 in Sessions").arg(sessions.size()));
    m_allSessions->setVisible(sessions.size()>order.size());
}
void ProjectsDialog::refreshLive()
{
    if(!isVisible()||!m_projects->group(currentId()))return;
    if(m_countsProvider){
        const auto counts=m_countsProvider();
        for(int row=0;row<m_list->count();++row){auto *item=m_list->item(row);const int count=counts.value(item->data(Qt::UserRole).toString());if(item->data(Qt::UserRole+4).toInt()!=count)item->setData(Qt::UserRole+4,count);}
    }
    m_projectSessions=m_sessionsProvider?m_sessionsProvider(currentId()):QList<ProjectSession>{};
    showSessions();
    // The kinds change in place: the folder rows, their order and the
    // selection stay.
    const auto kinds=folderKinds();const QSignalBlocker block(m_folders);
    for(int row=0;row<m_folders->rowCount();++row){
        auto *name=m_folders->item(row,0);auto *kind=m_folders->item(row,3);if(!name||!kind)continue;
        const auto now=kinds.value(name->data(Qt::UserRole).toString());
        if(name->data(Qt::UserRole+1).toBool()!=now.team)name->setData(Qt::UserRole+1,now.team);
        if(kind->text()!=now.text)kind->setText(now.text);
    }
    m_folders->viewport()->update();
}
void ProjectsDialog::showEvent(QShowEvent *event)
{
    QDialog::showEvent(event);
    // Polls skip a hidden page.
    refreshLive();
}
QString ProjectsDialog::folderKind(const SessionOrganization::Folder &folder, bool *team) const
{
    const auto kind=folderKinds().value(folder.id);
    if(team)*team=kind.team;
    return kind.text;
}
QHash<QString,ProjectsDialog::FolderKind> ProjectsDialog::folderKinds() const
{
    QHash<QString,FolderKind> kinds;
    const auto *project=m_projects->group(currentId());if(!project)return kinds;
    const auto key=[](const QString &path){return QDir::cleanPath(path).toCaseFolded();};
    // A session running in a folder (its working folder or its checkout root)
    // knows its branch and whether it is a worktree; the first one counts.
    // One pass over each machine's sessions (N23).
    struct Seen { QString branch, root; bool worktree = false; };
    QHash<QString,QHash<QString,Seen>> machines;
    for(const auto &folder:project->folders){
        if(machines.contains(folder.machine))continue;
        auto &paths=machines[folder.machine];
        const auto *box=folder.machine==m_fleet.local().host?&m_fleet.local():m_fleet.peer(folder.machine);
        if(!box)continue;
        for(const auto &session:box->sessions){
            const Seen seen{session.gitBranch,session.gitRoot,session.gitWorktree};
            const auto cwd=session.canonicalCwd.isEmpty()?session.cwd:session.canonicalCwd;
            if(!cwd.isEmpty()&&!paths.contains(key(cwd)))paths.insert(key(cwd),seen);
            if(!session.gitRoot.isEmpty()&&!paths.contains(key(session.gitRoot)))paths.insert(key(session.gitRoot),seen);
        }
    }
    // A team member's own worktree or clone: the generated
    // "<folder>-team-<id8>-<member>" path (dispatcher.rs member_folder_path),
    // or where a member session with its own folder runs.
    QSet<QString> teamFolders;
    for(const auto &session:m_projectSessions)
        if(session.teamFolder&&!session.cwd.isEmpty())teamFolders.insert(session.machine+'\n'+key(session.cwd));
    static const QRegularExpression generated(QStringLiteral("-team-([0-9a-f]{8})-([a-z0-9-]+)$"));
    bool mainCheckout=false;
    for(const auto &folder:project->folders){
        const auto match=generated.match(QDir::cleanPath(folder.path));
        FolderKind kind;kind.team=match.hasMatch()||teamFolders.contains(folder.machine+'\n'+key(folder.path));
        QString label,branch;
        const auto &paths=machines[folder.machine];const auto seen=paths.constFind(key(folder.path));
        if(seen!=paths.cend()){
            branch=seen->branch;
            // No checkout root: an ordinary folder without Git.
            label=seen->worktree?tr("Worktree"):seen->root.isEmpty()?tr("Folder"):QString();
        }else if(match.hasMatch()){
            // Worktree or clone, as the member's agent chose; no session tells which.
            label=tr("Team folder");branch=QStringLiteral("team/%1/%2").arg(match.captured(1),match.captured(2));
        }
        if(label.isEmpty()){
            // The first checkout of the project is its main checkout; the
            // others, and a team member's own clone, are clones.
            label=!kind.team&&!mainCheckout?tr("Main checkout"):tr("Clone");
            if(!kind.team)mainCheckout=true;
        }
        kind.text=branch.isEmpty()?label:tr("%1 · %2").arg(label,branch);
        kinds.insert(folder.id,kind);
    }
    return kinds;
}
void ProjectsDialog::changed()
{
    if (m_projects == &m_ownedProjects) {
        const QJsonDocument document(m_projects->toJson()); const auto bytes = document.toJson(QJsonDocument::Compact);
        SettingsJson::store("workspace/organization", bytes, document);
    }
    emit organizationChanged(); refresh(); setStatus(tr("Saved"));
}
void ProjectsDialog::selectProject(const QString &id)
{
    refresh(); for (int i=0;i<m_list->count();++i) if (m_list->item(i)->data(Qt::UserRole).toString()==id) {m_list->setCurrentRow(i);break;}
}
void ProjectsDialog::selectHost(const QString &host) { m_host=host; refresh(); }
void ProjectsDialog::setFleet(const FleetState &fleet)
{
    m_fleet=fleet; updateSwarmConnections();
    if (m_worktreeOverview) m_worktreeOverview->setContext(*m_projects, m_fleet, m_allProjects->isChecked());
    updateFolderActions();
    // N22: the Sessions rows and the folder kinds follow the fleet while the
    // page shows, at most once a second (after the window took the poll in).
    if (isVisible() && !m_live->isActive()) m_live->start();
    const auto request=[this](const QString &host,const QString &machine,bool online){
        if (!online || machine.isEmpty() || m_projects->hasImported(machine) || m_importing.contains(host)
            || m_importDeferred.contains(machine)) return;
        m_importing.insert(host);m_client->requestProjects(host);
    };
    // A control-only computer (hgs.exe on Windows) has no project folders of its own.
    if (fleet.local().localSessions) request({},fleet.local().host,fleet.local().ok);
    for (const auto &host:fleet.peerNames()) request(host,host,fleet.peer(host)->ok);
}
void ProjectsDialog::importLegacy(const QString &host,const QList<ProjectInfo> &projects)
{
    if (!m_importing.remove(host)) return;
    const auto machine=host.isEmpty()?m_fleet.local().host:host;
    if (machine.isEmpty() || m_projects->hasImported(machine)) return;
    const auto style=folderStyle(machine);
    QStringList rejected;
    for (const auto &legacy:projects) {
        if (legacy.fromAnsible) continue;
        // A folder this computer's form does not take never creates its project.
        if (!MachinePlatform::absolute(legacy.dir,style) || legacy.dir.contains('\n')) {rejected << legacy.name+" ("+legacy.dir+")";continue;}
        QString id; for(const auto &project:m_projects->groups()) if(project.name==legacy.name){id=project.id;break;}
        if(id.isEmpty()) id=m_projects->createGroup(legacy.name);
        if(m_projects->addFolder(id,machine,legacy.dir,{},style).isEmpty()) rejected << legacy.name+" ("+legacy.dir+")";
    }
    if (rejected.isEmpty()) m_projects->markImported(machine);
    else m_importDeferred.insert(machine);
    changed();
    if (!rejected.isEmpty())
        setStatus(tr("Could not add %n hgs project folder(s) of %1: %2","",rejected.size()).arg(machine,rejected.join(QStringLiteral(", "))),true);
}
void ProjectsDialog::setProjects(const QList<ProjectInfo> &projects) { importLegacy(m_host,projects); }
void ProjectsDialog::addProject()
{
    bool ok=false;const auto name=QInputDialog::getText(this,tr("New project"),tr("Name"),QLineEdit::Normal,{},&ok);
    if(!ok || name.trimmed().isEmpty())return;
    const auto id=m_projects->createGroup(name);changed();selectProject(id);
}
void ProjectsDialog::removeProject() { deleteProject(currentId()); }
void ProjectsDialog::deleteProject(const QString &id)
{
    if(!m_projects->group(id)||m_projects->groups().size()<2)return;
    QDialog dialog(this);dialog.setWindowTitle(tr("Delete project"));dialog.setObjectName("deleteProjectDialog");dialog.setMinimumWidth(480);
    auto *layout=new QVBoxLayout(&dialog);layout->setContentsMargins(24,22,24,20);layout->setSpacing(18);
    auto *hint=new QLabel(tr("Delete “%1”? Choose the project that will receive its sessions. Files and running agents stay in place.").arg(m_projects->group(id)->name));hint->setTextFormat(Qt::PlainText);hint->setWordWrap(true);hint->setMinimumWidth(360);layout->addWidget(hint);
    auto *target=new QComboBox;target->setObjectName("projectTransferTarget");
    for(const auto &p:m_projects->groups())if(p.id!=id)target->addItem(p.name,p.id);
    const int defaultRow=target->findData(m_projects->defaultProject());if(defaultRow>=0)target->setCurrentIndex(defaultRow);layout->addWidget(target);
    layout->addSpacing(4);
    auto *buttons=new QDialogButtonBox(QDialogButtonBox::Ok|QDialogButtonBox::Cancel);buttons->button(QDialogButtonBox::Ok)->setText(tr("Move sessions and delete"));layout->addWidget(buttons);
    buttons->button(QDialogButtonBox::Cancel)->setDefault(true);
    connect(buttons,&QDialogButtonBox::accepted,&dialog,&QDialog::accept);connect(buttons,&QDialogButtonBox::rejected,&dialog,&QDialog::reject);
    if(dialog.exec()!=QDialog::Accepted)return;
    if(m_projects->removeGroup(id,target->currentData().toString()))changed();
}
void ProjectsDialog::cloneRepository()
{
    QDialog choice(this);choice.setWindowTitle(tr("Clone repository"));choice.setObjectName("cloneRepositoryComputerDialog");
    auto *layout=new QVBoxLayout(&choice);auto *label=new QLabel(tr("Clone on computer"));layout->addWidget(label);
    auto *computer=new QComboBox;computer->setObjectName("cloneTargetComputer");
    if(m_fleet.local().localSessions&&m_fleet.local().ok)computer->addItem(m_fleet.local().host,QString());
    for(const auto &host:m_fleet.peerNames())if(const auto *box=m_fleet.peer(host);box&&box->ok)computer->addItem(host,host);
    layout->addWidget(computer);
    const auto suggested=selectedFolder().machine;
    const auto preferred=suggested.isEmpty()?m_host:suggested==m_fleet.local().host?QString():suggested;
    const auto index=computer->findData(preferred);if(index>=0)computer->setCurrentIndex(index);
    auto *buttons=new QDialogButtonBox(QDialogButtonBox::Ok|QDialogButtonBox::Cancel);buttons->button(QDialogButtonBox::Ok)->setText(tr("Continue"));buttons->button(QDialogButtonBox::Ok)->setEnabled(computer->count()>0);layout->addWidget(buttons);
    connect(buttons,&QDialogButtonBox::accepted,&choice,&QDialog::accept);connect(buttons,&QDialogButtonBox::rejected,&choice,&QDialog::reject);
    if(choice.exec()!=QDialog::Accepted)return;
    const auto host=computer->currentData().toString();const auto machine=host.isEmpty()?m_fleet.local().host:host;const auto style=folderStyle(machine);
    CloneRepositoryDialog dialog(m_client,host,machine,style,*m_projects,currentId(),false,this);
    if(dialog.exec()!=QDialog::Accepted)return;
    auto project=dialog.projectId();
    if(!project.isEmpty()&&!m_projects->group(project)){setStatus(tr("Project changed. Checkout kept at %1 on %2.").arg(dialog.createdPath(),machine),true);return;}
    if(project.isEmpty())project=m_projects->createGroup(dialog.newProjectName());
    if(m_projects->addFolder(project,machine,dialog.createdPath(),{},style).isEmpty()){setStatus(tr("Checkout kept at %1. Add its folder to the project.").arg(dialog.createdPath()),true);return;}
    changed();selectProject(project);
}

void ProjectsDialog::addFolder() { editFolder({}); }
void ProjectsDialog::editFolder(const QString &id, const QString &projectId)
{
    const auto project=projectId.isEmpty()?currentId():projectId;if(!m_projects->group(project))return;
    SessionOrganization::Folder existing;
    for(const auto &folder:m_projects->group(project)->folders)if(folder.id==id)existing=folder;
    if(!id.isEmpty()&&existing.id.isEmpty())return;
    QDialog dialog(this);dialog.setObjectName("projectFolderDialog");dialog.setWindowTitle(id.isEmpty()?tr("Add project folder"):tr("Edit project folder"));dialog.setMinimumWidth(540);
    auto *layout=new QVBoxLayout(&dialog);auto *form=new QFormLayout;
    auto *machine=new QComboBox;machine->setObjectName("projectFolderMachine");
    const auto local=m_fleet.local().host;
    if(!local.isEmpty()&&m_fleet.local().localSessions)machine->addItem(local+tr(" (this computer)"),QString());
    for(const auto &peer:m_fleet.peerNames())machine->addItem(peer,peer);
    const auto initialHost=existing.id.isEmpty()?m_host:existing.machine==local?QString():existing.machine;
    if(!initialHost.isEmpty()&&machine->findData(initialHost)<0)machine->addItem(initialHost,initialHost);
    machine->setCurrentIndex(qMax(0,machine->findData(initialHost)));
    auto *name=new QLineEdit;name->setObjectName("projectFolderName");name->setPlaceholderText(tr("Optional label"));name->setText(existing.name);
    auto *path=new QLineEdit;path->setObjectName("projectFolderPath");path->setPlaceholderText(tr("Absolute folder path"));path->setText(MachinePlatform::native(existing.path,folderStyle(existing.machine)));
    auto *row=new QHBoxLayout;row->addWidget(path,1);auto *browse=new QPushButton(tr("Browse…"));row->addWidget(browse);
    form->addRow(tr("Computer"),machine);form->addRow(tr("Name"),name);form->addRow(tr("Folder"),row);layout->addLayout(form);
    auto *buttons=new QDialogButtonBox(QDialogButtonBox::Ok|QDialogButtonBox::Cancel);buttons->button(QDialogButtonBox::Ok)->setText(id.isEmpty()?tr("Add folder"):tr("Save folder"));layout->addWidget(buttons);
    const auto validate=[&]{
        const auto host=machine->currentData().toString();const auto address=host.isEmpty()?local:host;
        // Drive and UNC folders only for a local Windows machine (D16); peers are POSIX.
        const auto style=folderStyle(address);
        bool duplicate=false;for(const auto &folder:m_projects->group(project)->folders)
            if(folder.id!=id&&folder.machine==address&&MachinePlatform::same(folder.path,MachinePlatform::clean(path->text(),style),style))duplicate=true;
        buttons->button(QDialogButtonBox::Ok)->setEnabled(!duplicate&&machine->currentIndex()>=0&&MachinePlatform::absolute(path->text(),style)&&!path->text().contains('\n'));
        path->setToolTip(duplicate?tr("This folder is already in this project."):QString());
    };
    connect(path,&QLineEdit::textChanged,&dialog,validate);validate();
    connect(browse,&QPushButton::clicked,&dialog,[&]{const auto chosen=DirectoryDialog::chooseDirectory(m_client->executable(),machine->currentData().toString(),path->text(),&dialog);if(!chosen.isEmpty())path->setText(chosen);});
    connect(machine,&QComboBox::currentIndexChanged,&dialog,[&]{path->clear();});
    connect(buttons,&QDialogButtonBox::accepted,&dialog,&QDialog::accept);connect(buttons,&QDialogButtonBox::rejected,&dialog,&QDialog::reject);
    if(dialog.exec()!=QDialog::Accepted)return;
    const auto host=machine->currentData().toString();const auto address=host.isEmpty()?local:host;
    if(id.isEmpty())m_projects->addFolder(project,address,path->text(),name->text(),folderStyle(address));
    else m_projects->editFolder(project,id,address,path->text(),name->text(),folderStyle(address));changed();
}
void ProjectsDialog::removeFolder(const QString &projectId, const QString &id)
{
    const auto present=[this,&projectId,&id]()->const SessionOrganization::Folder *{
        if(const auto *project=m_projects->group(projectId))for(const auto &folder:project->folders)if(folder.id==id)return &folder;
        return nullptr;
    };
    const auto *folder=id.isEmpty()?nullptr:present();if(!folder)return;
    // A destructive action asks first (N25); the files stay where they are.
    const auto style=folderStyle(folder->machine);
    const auto name=folder->name.isEmpty()?suggestName(folder->path,style):folder->name;
    QMessageBox confirm(QMessageBox::Question,tr("Remove folder?"),
        tr("Remove “%1” (%2 on %3) from %4? Files stay on disk.").arg(name,MachinePlatform::native(folder->path,style),folder->machine,m_projects->group(projectId)->name),
        QMessageBox::Cancel,this);
    confirm.setObjectName("removeProjectFolderConfirm");confirm.setTextFormat(Qt::PlainText);
    auto *remove=confirm.addButton(tr("Remove folder"),QMessageBox::DestructiveRole);remove->setObjectName("removeProjectFolderConfirmButton");
    confirm.setDefaultButton(QMessageBox::Cancel);confirm.setEscapeButton(QMessageBox::Cancel);
    confirm.exec();
    // The catalog can change while the question is open: read it again.
    if(confirm.clickedButton()!=remove||!present())return;
    m_projects->removeFolder(projectId,id);changed();
}
void ProjectsDialog::transferFolder(const QString &projectId, const SessionOrganization::Folder &folder,
                                    const QString &destination, bool copy)
{
    const auto *source = m_projects->group(projectId);
    bool present = false;
    if (source) for (const auto &current : source->folders)
        if (current.id == folder.id && current.machine == folder.machine && current.path == folder.path
            && current.name == folder.name && current.machineId == folder.machineId && current.machineName == folder.machineName) {
            present = true;
            break;
        }
    if (!present) { setStatus(tr("This project folder changed. Select it again."), true); return; }
    const auto *target = m_projects->group(destination);
    if (!target || destination == projectId || (!target->accessible && !m_allProjects->isChecked())) {
        setStatus(tr("The destination project is no longer available. Select it again."), true);
        return;
    }
    const auto name = target->name;
    const auto before = m_projects->toJson();
    const auto id = copy ? m_projects->copyFolder(projectId, folder.id, destination)
                         : m_projects->moveFolder(projectId, folder.id, destination);
    if (id.isEmpty()) { setStatus(tr("Could not transfer this project folder."), true); return; }
    if (before == m_projects->toJson()) {
        setStatus(tr("This folder is already in %1.").arg(name));
        return;
    }
    changed();
    setStatus(copy ? tr("Folder copied to %1.").arg(name) : tr("Folder moved to %1.").arg(name));
}
MachinePlatform::Style ProjectsDialog::folderStyle(const QString &machine) const
{
    return MachinePlatform::of(m_fleet.metricsOf(machine),!machine.isEmpty()&&machine==m_fleet.local().host);
}
SessionOrganization::Folder ProjectsDialog::selectedFolder() const
{
    if(m_folders->selectedItems().isEmpty())return {};
    const auto *item=m_folders->item(m_folders->currentRow(),0);
    const auto *project=m_projects->group(currentId());
    if(item&&project)for(const auto &folder:project->folders)if(folder.id==item->data(Qt::UserRole).toString())return folder;
    return {};
}
QString ProjectsDialog::folderOpenProblem(const SessionOrganization::Folder &folder) const
{
    if(folder.id.isEmpty())return tr("Select a folder to open.");
    if(folder.machine.isEmpty()||m_fleet.local().host.isEmpty())return tr("Waiting for this computer's identity.");
    if(folder.machine!=m_fleet.local().host)return tr("Open this folder from Zerus on %1.").arg(folder.machine);
    if(!QFileInfo(folder.path).isAbsolute())return tr("This folder is unavailable on this computer.");
    return localFolderProblem(folder.path);
}
QString ProjectsDialog::localFolderProblem(const QString &path) const
{
    // #744: never a stat on this thread; "checking" until the worker answers.
    const auto info=PathProbe::instance().info(path);
    if(info.state==PathProbe::Info::Checking)return tr("Checking this folder…");
    if(info.state==PathProbe::Info::Unresponsive)return tr("This folder does not answer: its disk may be busy.");
    if(!info.dir)return tr("This folder is unavailable on this computer.");
    return {};
}
void ProjectsDialog::updateFolderActions()
{
    const auto folder=selectedFolder(); const auto problem=folderOpenProblem(folder);
    m_worktrees->setVisible(!folder.id.isEmpty());
    fitFolders();
    m_worktrees->setContext(folder.machine==m_fleet.local().host?QString():folder.machine,folder.path,m_fleet);
    m_editFolder->setEnabled(!folder.id.isEmpty());m_removeFolder->setEnabled(!folder.id.isEmpty());
    m_openFolder->setEnabled(problem.isEmpty());
    const auto shown=MachinePlatform::native(folder.path,folderStyle(folder.machine));
    m_openFolder->setToolTip(problem.isEmpty()?tr("Open in file manager\n%1").arg(shown):problem);
    const auto launchProblem=folderLaunchProblem(folder);m_newSession->setEnabled(launchProblem.isEmpty());
    m_newSession->setToolTip(launchProblem.isEmpty()?tr("New session in %1 on %2").arg(shown,folder.machine):launchProblem);
}
QString ProjectsDialog::folderLaunchProblem(const SessionOrganization::Folder &folder) const
{
    if(folder.id.isEmpty())return tr("Select a folder for the new session.");
    if(folder.machine.isEmpty()||m_fleet.local().host.isEmpty())return tr("Waiting for this computer's identity.");
    const bool local=folder.machine==m_fleet.local().host;
    const auto *machine=local?&m_fleet.local():m_fleet.peer(folder.machine);
    if(!machine||!machine->ok)return tr("Reconnect %1 to start a session in this folder.").arg(folder.machine);
    if(!MachinePlatform::absolute(folder.path,folderStyle(folder.machine)))return tr("This folder is unavailable on this computer.");
    return local?localFolderProblem(folder.path):QString();
}
void ProjectsDialog::newFolderSession(const QString &projectId,const SessionOrganization::Folder &folder)
{
    const auto *project=m_projects->group(projectId);bool present=false;
    if(project)for(const auto &current:project->folders)
        if(current.id==folder.id&&current.machine==folder.machine&&current.path==folder.path){present=true;break;}
    if(!present){setStatus(tr("This project folder changed. Select it again."),true);return;}
    const auto problem=folderLaunchProblem(folder);if(!problem.isEmpty()){setStatus(problem,true);return;}
    emit newSessionRequested(projectId,folder.machine==m_fleet.local().host?QString():folder.machine,folder.id);
}
void ProjectsDialog::openFolder(const SessionOrganization::Folder &folder)
{
    const auto problem=folderOpenProblem(folder);
    if(!problem.isEmpty()){setStatus(problem,true);return;}
    // #744: a fresh answer from the worker before the file manager opens it.
    PathProbe::instance().check(folder.path,this,[this,path=folder.path](const PathProbe::Info &info){
        if(info.state==PathProbe::Info::Unresponsive){setStatus(tr("This folder does not answer: its disk may be busy."),true);return;}
        if(!info.dir){setStatus(tr("This folder is unavailable on this computer."),true);return;}
        if(!QDesktopServices::openUrl(QUrl::fromLocalFile(path)))setStatus(tr("Could not open the folder in the file manager."),true);
    });
}
void ProjectsDialog::showFolderMenu(const QPoint &position)
{
    const auto *item=position.x()<0?m_folders->currentItem():m_folders->itemAt(position);
    if(item)m_folders->setCurrentCell(item->row(),0);
    const auto folder=item?selectedFolder():SessionOrganization::Folder{};
    const auto project=currentId();
    auto *menu=new QMenu(this);menu->setObjectName("projectFolderMenu");menu->setToolTipsVisible(true);
    const QColor color(m_dark?"#a1adbb":"#647386");
    if(!folder.id.isEmpty()) {
        const auto shown=MachinePlatform::native(folder.path,folderStyle(folder.machine));
        auto *launch=menu->addAction(workspaceIcon("add",color),tr("New session…"));launch->setObjectName("contextNewProjectFolderSession");
        const auto launchProblem=folderLaunchProblem(folder);launch->setEnabled(launchProblem.isEmpty());launch->setToolTip(launchProblem.isEmpty()?shown:launchProblem);
        connect(launch,&QAction::triggered,this,[this,project,folder]{newFolderSession(project,folder);});
        menu->addSeparator();
        auto *open=menu->addAction(workspaceIcon("folder",color),tr("Open folder"));open->setObjectName("contextOpenProjectFolder");
        const auto problem=folderOpenProblem(folder);open->setEnabled(problem.isEmpty());open->setToolTip(problem.isEmpty()?shown:problem);
        // #744: the worker's answer reaches the open menu too.
        connect(&PathProbe::instance(),&PathProbe::answered,menu,[this,folder,launch,open,shown](const QString &path){
            if(path!=folder.path)return;
            const auto launchNow=folderLaunchProblem(folder);launch->setEnabled(launchNow.isEmpty());launch->setToolTip(launchNow.isEmpty()?shown:launchNow);
            const auto openNow=folderOpenProblem(folder);open->setEnabled(openNow.isEmpty());open->setToolTip(openNow.isEmpty()?shown:openNow);
        });
        connect(open,&QAction::triggered,this,[this,folder]{openFolder(folder);});
        auto *copy=menu->addAction(tr("Copy folder path"));copy->setObjectName("contextCopyProjectFolder");
        connect(copy,&QAction::triggered,this,[shown]{QApplication::clipboard()->setText(shown);});
        menu->addSeparator();
        for (const bool copyFolder : {false, true}) {
            auto *targets = menu->addMenu(copyFolder ? tr("Copy to") : tr("Move to"));
            targets->setObjectName(copyFolder ? "copyProjectFolderTo" : "moveProjectFolderTo");
            targets->setToolTipsVisible(true);
            for (const auto &target : m_projects->groups()) {
                if (target.id == project || (!target.accessible && !m_allProjects->isChecked())) continue;
                auto label = target.name;
                label.replace('&', "&&");
                auto *action = targets->addAction(workspaceIcon("projects", QColor(target.color)), label);
                action->setData(target.id);
                action->setToolTip(copyFolder ? tr("Add this folder to %1 on the same computer.").arg(target.name)
                                            : tr("Move this folder link to %1 on the same computer.").arg(target.name));
                connect(action, &QAction::triggered, this, [this, project, folder, destination = target.id, copyFolder] {
                    transferFolder(project, folder, destination, copyFolder);
                });
            }
            targets->setEnabled(!targets->actions().isEmpty());
        }
        menu->addSeparator();
        auto *edit=menu->addAction(tr("Edit folder…"));edit->setObjectName("contextEditProjectFolder");
        connect(edit,&QAction::triggered,this,[this,project,folder]{editFolder(folder.id,project);});
        auto *remove=menu->addAction(tr("Remove folder from project"));remove->setObjectName("contextRemoveProjectFolder");
        remove->setToolTip(tr("Files stay in place."));
        connect(remove,&QAction::triggered,this,[this,project,folder]{removeFolder(project,folder.id);});
        menu->addSeparator();
    }
    auto *add=menu->addAction(tr("Add folder…"));add->setObjectName("contextAddProjectFolder");add->setEnabled(!project.isEmpty());
    connect(add,&QAction::triggered,this,[this,project]{editFolder({},project);});
    connect(menu,&QMenu::aboutToHide,menu,&QObject::deleteLater);
    const auto point=position.x()<0&&item?m_folders->visualItemRect(item).center():position;
    menu->popup(m_folders->viewport()->mapToGlobal(point));
}
bool ProjectsDialog::nameProblem(const QString &name,QString *why)
{
    const bool bad=name.trimmed().isEmpty()||name.size()>80||name.contains('\n');
    if(why)*why=bad?tr("Use a name of 1–80 characters on one line."):QString();return bad;
}
QString ProjectsDialog::suggestName(const QString &dir, MachinePlatform::Style style)
{
    return MachinePlatform::leaf(MachinePlatform::clean(dir,style),style);
}

void ProjectsDialog::setTheme(bool dark)
{
    m_dark=dark;m_preview->setTheme(dark);m_list->setProperty("hgsDark",dark);m_list->viewport()->update();
    m_folders->setProperty("hgsDark",dark);m_folders->viewport()->setProperty("hgsDark",dark);m_folders->viewport()->update();
    m_openFolder->setIcon(workspaceIcon("folder",QColor(dark?"#a1adbb":"#647386")));
    m_folderSplitter->setProperty("hgsDark",dark);m_list->viewport()->setProperty("hgsDark",dark);
    // U25, #646: the page's cards, blocks and buttons from the skin - the
    // prototype's .btn.sm, .btn.ghost, .btn.pri and .btn.dng, its .ptbl rows
    // and the project list. Closer than the window's sheet, so they win there.
    const auto c=[dark](const char *name){return tok(dark,name).name(QColor::HexArgb);};
    QColor faded=tok(dark,"accent");faded.setAlphaF(.42);
    // The page's small buttons, and the selected folder's worktree actions.
    QString buttons;
    for(const auto &button:{QStringLiteral("QWidget#projectsSplit QPushButton[pageSm=\"true\"]"),QStringLiteral("QWidget#worktreePanel QPushButton")})
        buttons+=QStringLiteral("%1{background:%2;color:%3;border:1px solid %4;border-radius:7px;padding:2px 9px;min-height:17px;font-size:11px;}"
            "%1:hover{background:%5;color:%6;}"
            "%1:disabled{color:%7;}"
            "%1[pageGhost=\"true\"]{background:transparent;border-color:transparent;}"
            "%1[pageGhost=\"true\"]:hover{background:%5;}"
            "%1[pagePrimary=\"true\"]{background:%8;border-color:%8;color:%9;font-weight:600;}"
            "%1[pagePrimary=\"true\"]:hover{background:%10;border-color:%10;}"
            "%1[pagePrimary=\"true\"]:disabled{background:%11;border-color:transparent;color:%9;}"
            "%1[pageDanger=\"true\"]{color:%12;}")
            .arg(button,c("surface"),c("text2"),c("border"),c("hover"),c("fg"),c("dim"),c("accent"),c("accentText"))
            .arg(c("accentHover"),faded.name(QColor::HexArgb),c("danger"));
    setStyleSheet(PageCards::sheet(dark)+QStringLiteral(
        "QSplitter#projectsSplit::handle{background:%1;}"
        "QWidget#projectListColumn,QWidget#projectDetails{background:transparent;}"
        "QLabel#projectsHeading,QLabel#projectHeaderName{font-size:17px;font-weight:600;color:%2;}"
        "QLineEdit#projectName{font-size:15px;font-weight:600;padding:1px 6px;}"
        "QLabel#projectListHint,QLabel#projectSummary,QLabel#projectRulesHint{color:%3;}"
        "QLabel#projectListHint,QLabel#projectRulesHint,QLabel#projectSwarmStatus,QLabel#projectConnectionHint{font-size:11px;}"
        "QLabel#projectSwarmStatus,QLabel#projectConnectionHint{color:%4;}"
        "QCheckBox#allSwarmProjects,QCheckBox#defaultProject{color:%5;font-size:11.5px;background:transparent;}"
        "QLabel#projectIdLabel,QLabel#projectId{color:%3;}"
        "QListWidget#logicalProjects{background:transparent;border:0;outline:0;}"
        "QTableWidget#projectFolders,QTableWidget#projectSessions{background:transparent;border:0;outline:0;font-size:12px;color:%2;"
        "selection-background-color:transparent;selection-color:%2;}"
        "QTableWidget#projectFolders::item,QTableWidget#projectSessions::item{padding:0 8px;border:0;background:transparent;}"
        "QTableWidget#projectFolders::item:selected,QTableWidget#projectSessions::item:selected{background:transparent;}"
        "QTableWidget#projectFolders QHeaderView,QTableWidget#projectFolders QHeaderView::section{background:transparent;color:%6;border:0;padding:0 8px;font-size:11px;}"
        // The selected folder's worktrees speak the page's size.
        "QWidget#worktreePanel QLabel#heading{font-size:13px;font-weight:600;color:%2;}")
        .arg(c("cardBorder"),c("fg"),c("dim"),c("muted"),c("text2"),c("label"))
        +buttons);
    m_add->setIcon(workspaceIcon("add",tok(dark,"text2")));m_addFolder->setIcon(workspaceIcon("add",tok(dark,"text2")));
    m_newSession->setIcon(workspaceIcon("add",PageCards::token(dark,"accentText")));
    m_newTeam->setIcon(workspaceIcon("team",PageCards::token(dark,"text2")));
    for(auto *button:{m_newSession,m_newTeam,m_add,m_addFolder,m_openFolder})button->setIconSize(QSize(12,12));
    for(auto *block:{m_foldersBlock,m_sessionsBlock,m_rulesBlock})block->setDark(dark);
    m_sessions->setProperty("hgsDark",dark);m_sessions->viewport()->setProperty("hgsDark",dark);m_sessions->viewport()->update();
    showProject();
}
void ProjectsDialog::chooseColor()
{
    const auto id=currentId();const auto *project=m_projects->group(id);if(!project)return;
    const auto title=project->name;QColor chosen(project->color);
    QDialog dialog(this,Qt::Popup);dialog.setAttribute(Qt::WA_NoMouseReplay);dialog.setObjectName("projectColorDialog");dialog.setWindowTitle(tr("Project color"));dialog.setMinimumWidth(390);
    auto *layout=new QVBoxLayout(&dialog);layout->setContentsMargins(22,20,22,20);layout->setSpacing(16);
    auto *preview=new ProjectPreview;preview->setObjectName("projectFillPreview");preview->setTheme(m_dark);layout->addWidget(preview);
    auto *palette=new QGridLayout;palette->setSpacing(8);layout->addLayout(palette);
    const QStringList colors{"#ffcf33","#ff9234","#f85c63","#e45dbf","#a96bf5","#6677ff","#2b97ff","#23c5dd","#00aa7f","#41c86b","#a6cc38","#8796aa"};
    auto *hex=new QLineEdit(chosen.name());hex->setObjectName("projectColorHex");hex->setMaxLength(7);
    auto *style=new QComboBox;style->setObjectName("projectColorStyle");style->addItem(tr("Soft fill"),false);style->addItem(tr("Bright fill"),true);style->setCurrentIndex(style->findData(project->vivid));
    auto *form=new QFormLayout;form->addRow(tr("Color"),hex);form->addRow(tr("Style"),style);layout->addLayout(form);
    auto *custom=new QPushButton(tr("More colors…"));custom->setObjectName("projectCustomColor");layout->addWidget(custom,0,Qt::AlignLeft);
    auto *buttons=new QDialogButtonBox(QDialogButtonBox::Apply|QDialogButtonBox::Cancel);layout->addWidget(buttons);
    const auto update=[&]{const QColor value(hex->text());const bool valid=value.isValid()&&hex->text().size()==7&&hex->text().startsWith('#');buttons->button(QDialogButtonBox::Apply)->setEnabled(valid);if(valid){chosen=value;preview->setProject(title,chosen,style->currentData().toBool(),id==m_projects->defaultProject());}};
    for(int i=0;i<colors.size();++i){auto *swatch=new QPushButton;swatch->setFixedSize(44,32);swatch->setAccessibleName(colors[i]);swatch->setToolTip(colors[i]);swatch->setStyleSheet(QString("QPushButton {background:%1;border:1px solid %2;border-radius:6px;padding:0;} QPushButton:hover {border:2px solid %2;}").arg(colors[i],m_dark?"#ffffff":"#17212b"));palette->addWidget(swatch,i/6,i%6);connect(swatch,&QPushButton::clicked,&dialog,[=]{hex->setText(colors[i]);});}
    connect(hex,&QLineEdit::textChanged,&dialog,update);connect(style,&QComboBox::currentIndexChanged,&dialog,update);update();
    connect(custom,&QPushButton::clicked,&dialog,[&]{WorkspaceColorDialog picker(chosen,&dialog);connect(&picker,&QColorDialog::currentColorChanged,&dialog,[&](const QColor &c){preview->setProject(title,c,style->currentData().toBool(),id==m_projects->defaultProject());});if(picker.exec()==QDialog::Accepted)hex->setText(picker.selectedColor().name());update();});
    connect(buttons->button(QDialogButtonBox::Apply),&QPushButton::clicked,&dialog,&QDialog::accept);connect(buttons,&QDialogButtonBox::rejected,&dialog,&QDialog::reject);
    if(execWorkspaceColorPopup(dialog)!=QDialog::Accepted)return;
    m_projects->setColor(id,chosen.name());m_projects->setVivid(id,style->currentData().toBool());changed();
}
