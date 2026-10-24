#include "WindowTitleBar.h"
#ifdef Q_OS_WIN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <windowsx.h>
#include <dwmapi.h>
#endif

// Windows: a frameless top-level window with the native frame behavior kept.
// The client area covers the whole window; hit tests give the edges back to
// Windows for resizing and the title bar's empty space for moving.
namespace FramelessWindow {
#ifdef Q_OS_WIN
void apply(QWidget *window)
{
    const auto hwnd = reinterpret_cast<HWND>(window->winId());
    const auto style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    SetWindowLongPtrW(hwnd, GWL_STYLE, style | WS_THICKFRAME | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX);
    const MARGINS shadow{0, 0, 0, 1};
    DwmExtendFrameIntoClientArea(hwnd, &shadow);
    SetWindowPos(hwnd, nullptr, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
}

bool nativeEvent(QWidget *window, const WindowTitleBar *bar, void *message, qintptr *result)
{
    auto *msg = static_cast<MSG *>(message);
    const auto hwnd = msg->hwnd;
    if (msg->message == WM_NCCALCSIZE && msg->wParam) {
        // A maximized window spreads its hidden frame past the screen; keep it to the work area.
        if (IsZoomed(hwnd)) {
            auto &client = reinterpret_cast<NCCALCSIZE_PARAMS *>(msg->lParam)->rgrc[0];
            MONITORINFO monitor{sizeof(MONITORINFO)};
            if (GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &monitor)) {
                const auto &work = monitor.rcWork;
                client.left = qMax(client.left, work.left); client.top = qMax(client.top, work.top);
                client.right = qMin(client.right, work.right); client.bottom = qMin(client.bottom, work.bottom);
            }
        }
        *result = 0;
        return true;
    }
    if (msg->message == WM_NCHITTEST) {
        POINT point{GET_X_LPARAM(msg->lParam), GET_Y_LPARAM(msg->lParam)};
        RECT frame; GetWindowRect(hwnd, &frame);
        if (!IsZoomed(hwnd)) {
            const int edge = qMax(4, qRound(5 * window->devicePixelRatioF()));
            const bool left = point.x < frame.left + edge, right = point.x >= frame.right - edge;
            const bool top = point.y < frame.top + edge, bottom = point.y >= frame.bottom - edge;
            const LRESULT edges = top ? (left ? HTTOPLEFT : right ? HTTOPRIGHT : HTTOP)
                                : bottom ? (left ? HTBOTTOMLEFT : right ? HTBOTTOMRIGHT : HTBOTTOM)
                                : left ? HTLEFT : right ? HTRIGHT : HTNOWHERE;
            if (edges != HTNOWHERE) { *result = edges; return true; }
        }
        ScreenToClient(hwnd, &point);
        const qreal ratio = window->devicePixelRatioF();
        const QPoint logical(qRound(point.x / ratio), qRound(point.y / ratio));
        *result = bar && bar->isVisible() && bar->dragsAt(bar->mapFrom(window, logical)) ? HTCAPTION : HTCLIENT;
        return true;
    }
    return false;
}
#else
void apply(QWidget *) {}
bool nativeEvent(QWidget *, const WindowTitleBar *, void *, qintptr *) { return false; }
#endif
}
