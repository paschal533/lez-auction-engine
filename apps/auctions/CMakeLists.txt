cmake_minimum_required(VERSION 3.21)
project(AuctionsPlugin LANGUAGES CXX)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Qml Quick)
qt_standard_project_setup(REQUIRES 6.8)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found. Set LOGOS_MODULE_BUILDER_ROOT.")
endif()

# ui_qml module with a hand-written C++ backend (QtRO .rep view contract). The auction logic lives in
# the auction_engine core module (a dependency in metadata.json, reached via modules().auction_engine),
# so the app links no auction library of its own.
logos_module(
    NAME auctions
    REP_FILE src/AuctionsBackend.rep
    SOURCES
        src/AuctionsPluginInterface.h
        src/AuctionsPlugin.h
        src/AuctionsPlugin.cpp
        src/AuctionsBackend.h
        src/AuctionsBackend.cpp
)
