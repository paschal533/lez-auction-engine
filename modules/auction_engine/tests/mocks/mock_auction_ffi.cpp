#include <cstdlib>
#include <cstring>

#include <logos_clib_mock.h>

#include "auction_ffi.h"
#include "mock_ffi_state.h"

namespace mock_ffi {

State& state() {
    static State s;
    return s;
}

void reset() {
    state() = State{};
}

std::int64_t fakeNowMs() {
    return state().nowMs;
}

}  // namespace mock_ffi

namespace {

char* copyMockResponse(const char* function_name, const char* request) {
    LOGOS_CMOCK_RECORD(function_name);
    auto& s = mock_ffi::state();
    s.requests[function_name].emplace_back(request == nullptr ? "" : request);
    s.nowMs += s.costMs[function_name];

    const char* response = LOGOS_CMOCK_RETURN_STRING(function_name);
    if (response == nullptr) return nullptr;
    const std::size_t size = std::strlen(response) + 1;
    auto* copy = static_cast<char*>(std::malloc(size));
    if (copy == nullptr) return nullptr;
    std::memcpy(copy, response, size);
    return copy;
}

}  // namespace

extern "C" char* auction_ffi_version(const char* req) {
    return copyMockResponse("auction_ffi_version", req);
}

extern "C" char* auction_ffi_view(const char* req) {
    return copyMockResponse("auction_ffi_view", req);
}

extern "C" char* auction_ffi_prepare(const char* req) {
    return copyMockResponse("auction_ffi_prepare", req);
}

extern "C" char* auction_ffi_tx_begin(const char* req) {
    return copyMockResponse("auction_ffi_tx_begin", req);
}

extern "C" char* auction_ffi_tx_poll(const char* req) {
    return copyMockResponse("auction_ffi_tx_poll", req);
}

extern "C" char* auction_ffi_tx_end(const char* req) {
    return copyMockResponse("auction_ffi_tx_end", req);
}

extern "C" void auction_ffi_free(char* value) {
    LOGOS_CMOCK_RECORD("auction_ffi_free");
    std::free(value);
}
