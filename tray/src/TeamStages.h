#pragma once
#include "TeamModel.h"
#include "TeamWidgets.h"
#include <functional>

// WhiteWh fork: agent teams, U36 (design/agent-teams/pipelines). A task's
// pipeline on the desktop: the stage chip line of a Board card or a Layers
// row (the stage, its round, its member's agent and a track of the task's
// stages), the inspector's Pipeline section with every round's verdict and
// findings, and a member's stage queue. Review is violet, test blue, the
// final acceptance green; a return, the round limit and a held folder warn.
namespace TeamUi {

QColor stageColor(const Colors &c, const QString &kind);

// One chip: its glyph kind, text and color; a skipped stage is struck through.
struct StageChip {
    QString kind, text, tooltip, state;   // state: running, waiting, returned, passed, skipped, limit, held, accept
    QColor color;
    bool strike = false;
};

// What a card shows of its pipeline: the chips, the stage member's agent, the
// track (one color per segment: implement, each stage, the acceptance), a
// note that replaces the card's activity ("Attempt 2 · 3 findings") and
// whether its border warns.
struct StageLine {
    QList<StageChip> chips;
    QString agent;
    QList<QColor> track;
    QString note;
    bool warn = false;
    bool known() const { return !chips.isEmpty(); }
};
StageLine stageLine(const Teams::Team &team, const Teams::Task &task, const Colors &c);
// The line as a widget ("teamStageLine"), nullptr when the task shows none.
QWidget *stageLineWidget(const Teams::Team &team, const Teams::Task &task, const Colors &c);
// U36: the team's Board shows In review and In test once any task has a
// pipeline (a choice other than none, a snapshot or a stage status).
bool usesPipelines(const Teams::Team &team);
// "1st", "2nd", "3rd", "4th".
QString ordinal(int n);
// A column header's note: "1 waiting" when stages there wait for a member.
QString waitingNote(const Teams::Team &team, const QList<const Teams::Task *> &tasks);

// The inspector's Pipeline section ("teamTaskPipeline"); nullptr for a task
// without a pipeline snapshot.
QWidget *pipelineSection(const Teams::Team &team, const Teams::Task &task, const Colors &c, bool dark);

// A member card's stage queue ("teamMemberStageQueue"): its stage, then the
// waiting ones it may take; nullptr when the member takes no stages.
QWidget *memberStageQueue(const Teams::Team &team, const Teams::Member &member, const Colors &c, double now,
                          const std::function<void(int task)> &open);

}
