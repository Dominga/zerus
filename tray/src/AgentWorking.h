#pragma once
#include "Settings.h"
#include <QDateTime>
#include <QIcon>
#include <QPainter>
#include <QSettings>
#include <QTimer>
#include <QWidget>
#include <algorithm>
#include <cmath>
#include <optional>

#include "WorkspaceIcons.h"
#include "WorkspaceSkin.h"

// WhiteWh fork, U25 (spec 1.4): "the agent works". The agent works, not the
// task: where a model icon stands for a member who really runs a task (in
// progress, no problem, not paused, its computer online, the team running),
// the icon's tile gets an accent ring and three dots under it that rise in
// turn, a wave of about one second (2 px, the risen dot brighter). With
// Settings > Appearance > "Animate working sessions" off the dots stand still.
// paintTile() serves painted surfaces (graph nodes, delegates); AgentTile is
// the same as a widget. The tile follows the prototype's .tile / .tile.t16.
namespace AgentWorking {

constexpr int PeriodMs = 1050;   // the prototype's wkdots
constexpr int FrameMs = 40;      // a repaint while animating
constexpr int Below = 7;         // room the dots take under the tile

// Settings > Appearance: the window sets it from workspace/animateActivity.
inline std::optional<bool> &animatedValue() { static std::optional<bool> value; return value; }
inline bool animated()
{
    auto &value = animatedValue();
    if (!value) value = Settings::value(QStringLiteral("workspace/animateActivity"), true).toBool();
    return *value;
}
inline void setAnimated(bool on) { animatedValue() = on; }

// Where the wave is, 0..1, on one clock for every tile in the window.
inline double phase() { return double(QDateTime::currentMSecsSinceEpoch() % PeriodMs) / PeriodMs; }

// How risen dot `index` (0 left, 1 middle, 2 right) is at `phase`, 0..1.
inline double rise(int index, double phase)
{
    double distance = std::fabs(phase - index / 3.0);
    distance = std::min(distance, 1.0 - distance);
    const double x = std::max(0.0, 1.0 - distance * 3.0);
    return x * x * (3 - 2 * x);
}

// A model tile at `tile` (17 or 22 px), its agent logo, and while `working` the
// accent ring and the dots below it (Below px). Colors: the skin's tokens.
inline void paintTile(QPainter &p, const QRectF &tile, const QIcon &logo, bool dark, bool working, double wavePhase)
{
    const auto &skin = WorkspaceSkin::active();
    const QColor accent = WorkspaceSkin::color(skin, dark, QStringLiteral("accent"));
    const QColor cardBorder = WorkspaceSkin::color(skin, dark, QStringLiteral("cardBorder"));
    p.save();
    p.setRenderHint(QPainter::Antialiasing);
    QColor ring = cardBorder;
    if (working)   // color-mix(accent 70%, cardBorder)
        ring = QColor::fromRgbF(accent.redF() * .7 + cardBorder.redF() * .3, accent.greenF() * .7 + cardBorder.greenF() * .3,
                                accent.blueF() * .7 + cardBorder.blueF() * .3);
    const double radius = tile.width() < 20 ? 5 : 6;
    p.setPen(QPen(ring, 1)); p.setBrush(WorkspaceSkin::color(skin, dark, QStringLiteral("tile")));
    p.drawRoundedRect(tile.adjusted(.5, .5, -.5, -.5), radius, radius);
    const int logoSize = tile.width() < 20 ? 12 : 14;
    const QRectF logoRect(tile.center().x() - logoSize / 2.0, tile.center().y() - logoSize / 2.0, logoSize, logoSize);
    logo.paint(&p, logoRect.toAlignedRect());
    if (working) {
        const bool still = !animated();
        p.setPen(Qt::NoPen);
        for (int i = 0; i < 3; ++i) {
            const double up = still ? 0 : rise(i, wavePhase);
            QColor dot = accent; dot.setAlphaF(still ? 1.0 : .45 + .55 * up);
            p.setBrush(dot);
            p.drawEllipse(QPointF(tile.center().x() + (i - 1) * 5, tile.bottom() + 4.5 - 2 * up), 1.5, 1.5);
        }
    }
    p.restore();
}

}   // namespace AgentWorking

// The model tile as a widget: setAgent, setWorking, setDark. It repaints on
// its own while working and animated, and only while visible.
class AgentTile : public QWidget {
public:
    explicit AgentTile(int size = 17, QWidget *parent = nullptr) : QWidget(parent), m_size(size)
    {
        setObjectName("agentTile");
        setFixedSize(size, size + AgentWorking::Below);
        m_timer.setInterval(AgentWorking::FrameMs);
        QObject::connect(&m_timer, &QTimer::timeout, this, [this] { update(); });
    }
    void setAgent(const QString &agent) { m_agent = agent; update(); }
    void setDark(bool dark) { m_dark = dark; update(); }
    void setWorking(bool working)
    {
        m_working = working;
        setAccessibleDescription(working ? QStringLiteral("Working") : QString());
        sync(); update();
    }
    bool isWorking() const { return m_working; }
    // Whether the dots move now: working, visible and animations on.
    bool isAnimating() const { return m_timer.isActive(); }
    // Re-reads AgentWorking::animated() after Settings changed it.
    void refreshAnimation() { sync(); update(); }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        const QColor mono = WorkspaceSkin::color(WorkspaceSkin::active(), m_dark, QStringLiteral("text2"));
        QIcon logo = agentLogoIcon(m_agent, mono);
        if (logo.isNull()) logo = workspaceIcon(QStringLiteral("terminal"), mono);
        AgentWorking::paintTile(p, QRectF(0, 0, m_size, m_size), logo, m_dark, m_working, AgentWorking::phase());
    }
    void showEvent(QShowEvent *) override { sync(); }
    void hideEvent(QHideEvent *) override { sync(); }

private:
    void sync()
    {
        const bool run = m_working && isVisible() && AgentWorking::animated();
        if (run && !m_timer.isActive()) m_timer.start();
        if (!run) m_timer.stop();
    }
    int m_size;
    QString m_agent;
    bool m_dark = true, m_working = false;
    QTimer m_timer;
};
