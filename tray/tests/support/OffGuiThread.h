#pragma once

#include <thread>
#include <type_traits>

// Runs f on a separate thread and returns its result. GUI suites call parsers
// that production runs on workers through this: JsonParse.h fails a parse on
// the GUI thread (ZERUS_GUI_THREAD_CHECKS=fatal in every suite).
template <typename F>
auto offGuiThread(F f)
{
    std::invoke_result_t<F &> result{};
    std::thread([&] { result = f(); }).join();
    return result;
}
