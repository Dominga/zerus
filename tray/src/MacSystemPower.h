#pragma once

#include <functional>
#include <memory>

// System sleep and wake notifications for MainThreadWatchdog. They arrive on a
// serial dispatch queue of their own, never on the main thread, which macOS may
// not schedule during the transition into sleep or a dark wake. Sleep is never
// vetoed: queries and sleep notices are acknowledged at once.
class MacSystemPower {
public:
    MacSystemPower(std::function<void()> willSleep, std::function<void()> poweredOn);
    ~MacSystemPower();

    MacSystemPower(const MacSystemPower &) = delete;
    MacSystemPower &operator=(const MacSystemPower &) = delete;

    struct State;

private:
    std::unique_ptr<State> m_state;
};
