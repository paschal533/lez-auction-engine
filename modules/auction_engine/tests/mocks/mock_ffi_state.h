#pragma once

#include <cstdint>
#include <map>
#include <string>
#include <vector>

// State of the mocked auction_ffi, shared by the mock and the tests. The mock keeps the requests it got
// and advances a fake clock by `costMs[function]` per call, so budget tests need no real waiting.
namespace mock_ffi {

struct State {
    std::int64_t nowMs = 0;
    std::map<std::string, std::int64_t> costMs;
    std::map<std::string, std::vector<std::string>> requests;
};

State& state();
void reset();
std::int64_t fakeNowMs();

}  // namespace mock_ffi
