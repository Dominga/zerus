#include "Settings.h"
#include "WindowLayer.h"
#include <QEvent>
#include <QGuiApplication>
#include <QSettings>
#include <QWindow>
#ifdef Q_OS_LINUX
#include "KWinWindowLayer.h"
#endif

WindowLayer::WindowLayer(QWidget *window)
    : QObject(window), m_window(window), m_wayland(QGuiApplication::platformName().startsWith("wayland")), m_poll(this)
{
    m_desired = Settings::value("workspace/alwaysOnTop", false).toBool();
    if (!m_wayland) { m_supported = true; m_onTop = m_desired; applyQt(m_onTop); return; }
#ifdef Q_OS_LINUX
    m_kwin = new KWinWindowLayer(this);
    m_window->installEventFilter(this);
    connect(m_kwin, &KWinWindowLayer::finished, this, [this](const QString &id, bool on, const QString &error) {
        const bool polling = m_polling; m_polling = false;
        m_busy = false;
        if (error == "missing" && m_id.isEmpty() && m_window->isVisible() && ++m_attempts < 8) {
            const auto epoch = m_epoch;
            m_busy = true; QTimer::singleShot(200, this, [this, epoch] { if (epoch == m_epoch) { m_busy = false; probe(); } }); return;
        }
        m_supported = !id.isEmpty(); m_id = id;
        if (!polling || !error.isEmpty() || on != m_onTop) m_error = error;
        if (m_supported) {
            m_poll.start();
            m_onTop = on;
            if (m_restore) {
                m_restore = false;
                if (m_desired != on) { request(m_desired); return; }
            }
            if (Settings::value("workspace/alwaysOnTop", false).toBool() != on)
                Settings::set("workspace/alwaysOnTop", on);
            if (m_queued) { const bool desired = *m_queued; m_queued.reset(); request(desired); return; }
        } else {
            m_poll.stop(); m_queued.reset();
        }
        emit changed();
    });
    m_poll.setInterval(1500);
    connect(&m_poll, &QTimer::timeout, this, [this] {
        if (m_busy || !m_supported || !m_window->isVisible()) return;
        m_busy = true; m_polling = true; m_kwin->inspect(m_id);
    });
    if (m_window->isVisible()) QTimer::singleShot(100, this, &WindowLayer::probe);
#endif
}

QString WindowLayer::hint() const
{
    if (m_busy && m_id.isEmpty()) return tr("Checking whether this desktop supports keeping Zerus above other windows…");
    if (m_error == "denied") return tr("KWin did not apply this preference. Check the window's Keep Above rule in KDE settings.");
    if (m_error == "ambiguous") return tr("Zerus could not uniquely identify its window in KWin. Close duplicate workspace windows and reopen Zerus from the tray to try again.");
    if (!m_supported) return tr("Keep Above is unavailable in this Zerus session. On Wayland, KDE Plasma 6 with KWin scripting is required. You can use your desktop's window menu or rules.");
    return tr("Files and terminals opened from Zerus may appear behind it while this is on.");
}

void WindowLayer::applyQt(bool on)
{
    // QWidget::setWindowFlag hides visible windows. Change the native flags in
    // place, and retain them on the widget for a later native re-creation.
    auto flags = m_window->windowFlags(); flags.setFlag(Qt::WindowStaysOnTopHint, on);
    if (flags == m_window->windowFlags()) return;
    m_window->overrideWindowFlags(flags);
    if (auto *handle = m_window->windowHandle()) handle->setFlags(flags);
}

void WindowLayer::request(bool on)
{
    if (m_busy && m_polling && m_supported) { m_queued = on; emit changed(); return; }
    if (!m_supported || m_busy) { emit changed(); return; }
    m_desired = on;
    if (!m_wayland) {
        applyQt(on); m_onTop = on; Settings::set("workspace/alwaysOnTop", on); emit changed(); return;
    }
#ifdef Q_OS_LINUX
    m_busy = true; emit changed(); m_kwin->request(m_window->windowTitle(), m_id, on);
#endif
}

void WindowLayer::probe()
{
#ifdef Q_OS_LINUX
    if (!m_kwin || m_busy || !m_window->isVisible()) return;
    m_busy = true; emit changed(); m_kwin->request(m_window->windowTitle(), {}, std::nullopt);
#endif
}

bool WindowLayer::eventFilter(QObject *object, QEvent *event)
{
    if (object == m_window && (event->type() == QEvent::Show || event->type() == QEvent::Hide)) {
        const auto epoch = ++m_epoch;
        m_poll.stop(); m_busy = false; m_polling = false; m_queued.reset();
#ifdef Q_OS_LINUX
        if (m_kwin) m_kwin->cancel();
#endif
        m_id.clear(); m_supported = false; m_attempts = 0;
        if (event->type() == QEvent::Show) {
            m_restore = true; m_desired = Settings::value("workspace/alwaysOnTop", false).toBool();
            QTimer::singleShot(100, this, [this, epoch] { if (epoch == m_epoch) probe(); });
        }
    }
    return QObject::eventFilter(object, event);
}
