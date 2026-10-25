cmake_minimum_required(VERSION 3.14)
project(AuctionEngineModuleTests LANGUAGES CXX)

include(LogosTest)

logos_test(
    NAME auction_engine_module_tests
    MODULE_SOURCES
        ../src/auction_engine_impl.cpp
        ../src/lez_args.cpp
    TEST_SOURCES
        main.cpp
        lez_args_test.cpp
        submit_test.cpp
        view_test.cpp
    MOCK_C_SOURCES
        mocks/mock_auction_ffi.cpp
    GENERATED_SOURCES
        ../generated_code/lez_core_api.cpp
    EXTRA_INCLUDES
        ../src
        mocks
        ../generated_code
)
