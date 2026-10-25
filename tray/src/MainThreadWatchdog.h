#pragma once

#include <QTimer>

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <functional>
#include <mutex>
#include <thread>

// Сторож главного потока: если тот перестал крутить цикл событий дольше порога, из
// отдельного потока зовётся onHang -- в бою это abort() (см. main.cpp), чтобы launchd
// (KeepAlive Crashed=true, см. packaging/com.hgdev.hgs-tray.plist) или systemd
// (Restart=on-failure, см. packaging/hgs-tray.service) подняли трей заново.
//
// Зачем. На macOS 26 главный поток трея может намертво повиснуть внутри AppKit при
// обновлении элемента строки меню -- см. QuietTrayIcon.h, там подробности и живой случай
// 2026-09-14: шесть дней иконка висела и не отвечала на клики, процесс при этом «работал».
// QuietTrayIcon режет поводы, но баг Apple'овский, и гарантии нет. Мёртвый трей, который
// перезапустился через полминуты, лучше мёртвого трея на неделю; а крэш-репорт от abort()
// ещё и показывает, где именно стоял главный поток (поток сторожа назван hgs-watchdog,
// чтобы в репорте было видно, кто нажал).
//
// Устройство. QTimer на главном потоке каждые threshold/4 пишет «я жив» (момент по
// nowMs()) в atomic; сторожевой std::thread каждые threshold/2 сравнивает с текущим
// моментом. Срабатывает не больше одного раза. Деструктор останавливает поток через
// condition_variable, а не ждёт очередной проверки.
//
// nowMs() must stop while the machine sleeps, or waking up after a night looks
// like a hang. On Linux steady_clock (CLOCK_MONOTONIC) already does. On macOS
// libc++'s steady_clock is CLOCK_MONOTONIC_RAW, which counts sleep, so nowMs()
// reads uptime (CLOCK_UPTIME_RAW, mach_absolute_time) there.
//
// Around sleep macOS leaves the main thread unscheduled for longer than the
// threshold while the rest of the process runs: during the transition into sleep
// and in a dark wake on battery. On 2026-10-22 a 14 s dark wake and an 18 s
// transition were enough; the watchdog aborted 0.4 s after the next wake while the
// main thread was already running timers again. systemWillSleep() and
// systemPoweredOn() (MacSystemPower.h) suspend the check from the sleep notification
// until the main thread has run after a wake, for wakeGraceMs of uptime at most, so
// a main thread that never runs again after a wake still counts as hung.
//
// Что НЕ считается зависанием: модальные диалоги (QMessageBox::exec крутит вложенный цикл,
// таймер тикает), любая обычная работа. Порог в десятки секунд с запасом покрывает даже
// очень тормозящий бокс.
//
// A main thread starved of CPU trips it too, whatever its stack shows. On macOS
// check `launchctl print gui/<uid>/com.hgdev.hgs-tray` first: a LaunchAgent
// without ProcessType Interactive runs as a daemon clamped below ordinary load.
//
// Windows has no steady clock that stops during sleep, so nowMs() reads the
// unbiased interrupt time there. Nothing supervises the tray on Windows; main.cpp
// registers it with Windows Error Reporting (RegisterApplicationRestart) instead,
// which restarts it after the abort once it has run for at least a minute.
class MainThreadWatchdog {
public:
    MainThreadWatchdog(int thresholdMs, std::function<void()> onHang,
                       int wakeGraceMs = 10 * 60 * 1000);
    ~MainThreadWatchdog();

    MainThreadWatchdog(const MainThreadWatchdog &) = delete;
    MainThreadWatchdog &operator=(const MainThreadWatchdog &) = delete;

    // Any thread. The system is about to sleep: hold the check until it powers on.
    void systemWillSleep();
    // Any thread, also for a dark wake: hold the check until the main thread beats.
    void systemPoweredOn();

private:
    using Clock = std::chrono::steady_clock;
    static std::int64_t nowMs();

    const int m_thresholdMs;
    const int m_wakeGraceMs;
    std::function<void()> m_onHang;
    std::atomic<std::int64_t> m_lastBeatMs;
    std::atomic<std::int64_t> m_powerEventMs{0};
    std::atomic<bool> m_sleeping{false};
    std::atomic<bool> m_awaitingBeat{false};
    QTimer m_heartbeat;
    std::mutex m_mutex;
    std::condition_variable m_wake;
    bool m_stop = false;
    // Последним: стартует в конструкторе, когда всё выше уже готово.
    std::thread m_thread;
};
