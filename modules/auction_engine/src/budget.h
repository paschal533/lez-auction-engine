#pragma once

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>

// The time budget of one module call (Ruling R10). Logos IPC calls between modules time out after 20 s, so a
// module method that makes several auction_ffi calls and a lez_core send shares one budget of 15 s. Every FFI
// call gets what is left as `ctx.budget_ms` and is not started below MIN_FFI_START_MS. The lez_core send is an
// IPC call this module cannot bound, so it is started only with MIN_SEND_START_MS left.
namespace auction_engine {

constexpr std::int64_t MODULE_CALL_BUDGET_MS = 15000;
constexpr std::int64_t MIN_FFI_START_MS = 1000;
constexpr std::int64_t MIN_SEND_START_MS = 3000;

using ClockFn = std::int64_t (*)();

inline std::atomic<ClockFn>& clockSlot() {
    static std::atomic<ClockFn> slot{nullptr};
    return slot;
}

/// Replaces the clock (milliseconds) so tests need no real waiting; nullptr restores the steady clock.
inline void setClockForTests(ClockFn clock) {
    clockSlot().store(clock);
}

inline std::int64_t nowMs() {
    if (const ClockFn clock = clockSlot().load()) return clock();
    return std::chrono::duration_cast<std::chrono::milliseconds>(
               std::chrono::steady_clock::now().time_since_epoch())
        .count();
}

/// Starts when it is constructed, at the top of a public module method.
class Budget {
public:
    Budget() : startMs_(nowMs()) { }

    std::int64_t remainingMs() const {
        return std::max<std::int64_t>(0, MODULE_CALL_BUDGET_MS - (nowMs() - startMs_));
    }

    bool hasAtLeast(std::int64_t ms) const { return remainingMs() >= ms; }

private:
    std::int64_t startMs_;
};

}  // namespace auction_engine
