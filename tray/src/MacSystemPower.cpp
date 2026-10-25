#include "MacSystemPower.h"

#include <IOKit/IOMessage.h>
#include <IOKit/pwr_mgt/IOPMLib.h>
#include <dispatch/dispatch.h>

#include <cstdint>
#include <cstdio>

struct MacSystemPower::State {
    std::function<void()> willSleep, poweredOn;
    io_connect_t root = MACH_PORT_NULL;
    IONotificationPortRef port = nullptr;
    io_object_t notifier = 0;
    dispatch_queue_t queue = nullptr;
};

namespace {
void received(void *context, io_service_t, natural_t message, void *argument)
{
    auto *state = static_cast<MacSystemPower::State *>(context);
    switch (message) {
    case kIOMessageCanSystemSleep:
        // An unanswered query delays idle sleep by 30 s.
        IOAllowPowerChange(state->root, reinterpret_cast<std::intptr_t>(argument));
        break;
    case kIOMessageSystemWillSleep:
        state->willSleep();
        IOAllowPowerChange(state->root, reinterpret_cast<std::intptr_t>(argument));
        break;
    case kIOMessageSystemWillPowerOn:
    case kIOMessageSystemHasPoweredOn:
        state->poweredOn();
        break;
    default:
        break;
    }
}
}

MacSystemPower::MacSystemPower(std::function<void()> willSleep, std::function<void()> poweredOn)
    : m_state(std::make_unique<State>())
{
    m_state->willSleep = std::move(willSleep);
    m_state->poweredOn = std::move(poweredOn);
    m_state->root = IORegisterForSystemPower(m_state.get(), &m_state->port, received,
                                             &m_state->notifier);
    if (m_state->root == MACH_PORT_NULL) {
        std::fprintf(stderr, "hgs-tray: cannot observe system sleep; the main-thread "
                             "watchdog may abort after a wake\n");
        return;
    }
    m_state->queue = dispatch_queue_create("hgs-system-power", DISPATCH_QUEUE_SERIAL);
    IONotificationPortSetDispatchQueue(m_state->port, m_state->queue);
}

MacSystemPower::~MacSystemPower()
{
    if (m_state->root == MACH_PORT_NULL)
        return;
    IODeregisterForSystemPower(&m_state->notifier);
    IOServiceClose(m_state->root);
    IONotificationPortDestroy(m_state->port);
    // A notification already on the queue finishes before the callbacks go away.
    dispatch_sync_f(m_state->queue, nullptr, [](void *) {});
    dispatch_release(m_state->queue);
}
