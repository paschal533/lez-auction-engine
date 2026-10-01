# Research 4: Basecamp packaging and the reference implementation for an RFP-014 auction engine

Date: 2026-10-01. Read-only research. Remote repos were shallow-cloned into `scratchpad/repos/`.

## 0. Pinned sources (commit SHAs)

| Repo | SHA (HEAD read) | Notes |
|---|---|---|
| logos-blockchain/lez-programs | `64c01210adecd6b557dbd021a3cc94553f243fd8` | reference monorepo (programs + modules + apps) |
| logos-co/logos-module-builder | `4b7998272c5ec014bcac4bf1c7dbe7602c63a3c1` (2026-09-23) | `lib.version = "0.2.0"` in lib/default.nix |
| logos-blockchain/logos-execution-zone-module (`lez_core`) | `b89e5d24df5babc2490d2e5d47756fb1f33435dd` (HEAD, metadata 0.5.0, LEZ 0.3.0) | see section 3 for the three API generations |
| logos-co/logos-modules-release-base | `33fb564d2069be388ef53724bd5b0a6d026b2250` | fork-me catalog template |
| logos-co/logos-modules-release-action | `bbdc52871dc4305c8007923e1b08738ad2dd6b50` | reusable `release.yml@v1` |
| logos-co/logos-standalone-app | `9afc7310baa739397e1d80725dd6345c0b2176f8` | `nix run` host + `lib.mkPluginTest` |
| logos-co/logos-design-system | `88330d04968954be956311b0be429048a41da2ea` | Logos.Theme / Logos.Controls / Logos.Icons |
| logos-blockchain/logos-execution-zone-wallet-ui | `4e49d9cead1732e5c6089690d3eed4b0aff2d0f5` | `lez_wallet_ui` (design-system consumer) |
| logos-co/nix-bundle-lgx | `d3445f62f9b0b1b3b8ca2193589c1051ff9e1a9e` | |
| logos-co/logos-scaffold | `f63d70f75bd0809c340db535a189173ab32ba506` | `lgs basecamp` |
| logos-co/logos-qt-mcp | `1fea509485a48ab185fafae8dc21bdbcc808a07e` | inspector + test framework |
| local logos-basecamp | `01d3f598ad1c360f309e31af4d5ffd65a6d8bd81` | latest tag 0.2.1 |

---

## 1. Reference implementation: lez-programs

### 1.1 Repo layout we should mirror

```
lez-programs/
├── Cargo.toml                 # one workspace: programs/*, modules/*/ffi, tools/idl-gen
│                              # guests EXCLUDED (programs/*/methods/guest is its own workspace)
├── flake.nix                  # crane builds each *_ffi; mkLogosModule/mkLogosQmlModule for modules + apps
├── Makefile                   # build-programs (docker guest build), clippy, test (RISC0_DEV_MODE=1), idl
├── artifacts/<program>-idl.json   # CI-checked IDL (cargo run -p idl-gen -- <guest bin.rs>)
├── programs/stablecoin/
│   ├── core/src/lib.rs        # Instruction enum + account structs (borsh), PDA seeds
│   ├── src/*.rs               # host-side program logic, one file per instruction, tests.rs
│   ├── methods/{build.rs,Cargo.toml,src/lib.rs}   # risc0_build::embed_methods()
│   └── methods/guest/{Cargo.toml,src/bin/stablecoin.rs}  # #[lez_program] SPEL entry
├── modules/stablecoin/        # universal core module
│   ├── metadata.json  flake.nix  CMakeLists.txt  README.md
│   ├── ffi/{Cargo.toml,build.rs,cbindgen.toml,include/stablecoin_ffi.h,src/{lib,ffi,account}.rs,src/api/*.rs,tests/public_api.rs}
│   ├── src/stablecoin_module_impl.{h,cpp}  src/stablecoin_module_support.{h,cpp}
│   └── tests/{CMakeLists.txt,main.cpp,logos_sdk.h,mocks/mock_stablecoin_ffi.cpp,*_test.cpp}
├── modules/amm/ (same shape; 1578-line impl; no tests dir)
├── apps/amm/                  # ui_qml app with C++ backend (classic hand-written plugin)
│   ├── metadata.json flake.nix CMakeLists.txt README.md VALIDATION.md
│   ├── src/{AmmUiBackend.rep,AmmUiBackend.{h,cpp},AmmUiPlugin.{h,cpp},AmmUiPluginInterface.h,RegistryLoader.{h,cpp}}
│   ├── qml/{Main.qml, chrome/, pages/, components/{liquidity,shared,swap}/, state/}
│   └── tests/{swap,create-pool,add-liquidity,remove-liquidity,custom-token,faucet-swap}.mjs
│        tests/qml/tst_*.qml   tests/testnet/setup-amm-testnet.sh
└── apps/shared/wallet/        # Logos.Wallet QML module + logos_wallet_access static lib
    ├── CMakeLists.txt
    ├── qml/{WalletControl,ProgramAccountSelector,TransactionConfirmationDialog,SubmittedTransaction}.qml
    ├── qml/internal/{AccountDelegate,CopyButton,CreateAccountDialog,CreateWalletDialog,WalletIconButton,WalletMessageDialog}.qml + icons/*.svg
    ├── src/{WalletProvider,LogosWalletProvider,WalletAccountModel,WalletController}.{h,cpp}
    └── tests/{cpp/LogosWalletProviderTest.cpp, cpp/fixtures/logos_sdk.h, qml/main.cpp, qml/tst_*.qml, support/FakeWalletProvider.h}
```

### 1.2 Core module (`modules/stablecoin`, `modules/amm`)

`modules/stablecoin/metadata.json`:
```json
{ "name": "stablecoin_module", "version": "0.1.0", "type": "core", "interface": "universal",
  "category": "stablecoin", "main": "stablecoin_module_plugin", "dependencies": ["lez_core"],
  "nix": { "packages": {"build": [], "runtime": []},
           "external_libraries": [ { "name": "stablecoin_ffi" } ],
           "cmake": { "find_packages": [], "extra_sources": [], "extra_include_dirs": [], "extra_link_libraries": [] } } }
```
(amm adds `"display_name": "Logos DEX Core"`, `"author"`, `"description"`.)

`CMakeLists.txt` (both modules):
```cmake
include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
file(READ "${CMAKE_CURRENT_SOURCE_DIR}/metadata.json" METADATA_JSON)
string(JSON MODULE_NAME GET ${METADATA_JSON} name)
logos_module(NAME ${MODULE_NAME}
    SOURCES src/stablecoin_module_impl.h src/stablecoin_module_impl.cpp ...
    EXTERNAL_LIBS stablecoin_ffi)
```
The `lez_core` dependency is NOT linked; it is declared in metadata.json and reached via `modules().lez_core`.

Impl header pattern (Qt-free, std types only, this header IS the API; the builder derives `.lidl` from it):
```cpp
#include <logos_json.h>            // LogosMap / LogosList (nlohmann::json aliases)
#include <logos_module_context.h>  // LogosModuleContext base + modules()
class StablecoinModuleImpl : public LogosModuleContext {
public:
    /// doc comment becomes the method description in getMethods()/Basecamp
    LogosMap programInfo();
    LogosMap initializeProgram(const LogosMap& request);
private: ...
};
```
Impl .cpp pattern:
```cpp
#include "logos_sdk.h"            // generated; only in .cpp, never in the impl header
extern "C" {
#include "stablecoin_ffi.h"
}
LogosMap publicOk()  { return {{"status","ok"},{"error",""}}; }
LogosMap publicError(const std::string& e) { return {{"status","error"},{"error",e}}; }
template <typename Op> LogosMap guarded(Op&& op) noexcept { try { return op(); } catch (...) {} return publicError("backend_error"); }
// FFI call: std::unique_ptr<char, decltype(&stablecoin_free)> resp(op(payload.c_str()), &stablecoin_free);
// parse {"ok":bool,"value":{...}} | {"ok":false,"error":"code"}
```
Submission (amm_module_impl.cpp line ~499):
```cpp
const std::vector<uint8_t> instruction = jsonWordsToLeBytes(plan.value("instruction", json::array()));
const std::string reply = modules().lez_core.send_generic_public_transaction(accounts, signers, instruction, program_id);
const auto obj = json::parse(reply, nullptr, false);
if (!obj.is_object() || !obj.value("success", false)) return error("wallet_submission_failed");
return LogosMap{{"status","ok"},{"error",""},{"transactionId", jStr(obj, "tx_hash")}};
```
Conventions worth copying:
- Every method returns `{status:"ok"|"error", error:<stable code>, ...}`; stable string error codes (`config_missing`, `invalid_account_id`, `wallet_submission_failed`, `backend_error`, ...).
- Big integers (u128 amounts, u64 unix-ms deadlines) are taken as `nlohmann::json` and accepted as JSON integer OR decimal string; JSON floats rejected (logoscore promotes bare numbers above about 2^31 to doubles).
- Program identity comes from env on the host process: `STABLECOIN_PROGRAM_ID` / `STABLECOIN_PROGRAM_BIN`, `AMM_PROGRAM_BIN`, `AMM_CONFIG_ID`; a `setAmmProgramId` / `setConfigId` method lets the UI inject the id from its registry. `*_DEBUG` env var gates stderr tracing (module runs in its own `logos_host` process).

### 1.3 Rust FFI cdylib (JSON in, JSON out)

`modules/stablecoin/ffi/Cargo.toml`: `crate-type = ["cdylib", "rlib"]`, `[build-dependencies] cbindgen = "0.28"`, deps `lee_core` (workspace, LEZ tag v0.2.4), `risc0-zkvm`, `borsh`, `serde_json`, program `*_core` crates. `build.rs` runs cbindgen to `include/<crate>.h` (committed). `cbindgen.toml`: `language="C"`, `include_guard`, `pragma_once`, `cpp_compat=true`.

Exported ABI (one fn per operation + a free):
```c
char *stablecoin_program_info(const char *request_json);
char *stablecoin_decode_protocol_parameters(const char *request_json);
char *stablecoin_initialize_program_plan(const char *request_json);
void  stablecoin_free(char *value);
```
`src/ffi.rs` wraps every op in `catch_unwind`, parses the request with serde (`bad_request` on failure), and returns envelope `{"ok":true,"value":{...}}` or `{"ok":false,"error":"<code>"}` (`backend_error` on panic). Plans return `{programId, accountIds[], signingRequirements[], instruction:[u32 words]}` which the C++ module converts to LE bytes. `tests/public_api.rs` asserts the crate-root re-export surface; `src/api/tests.rs` holds unit tests.

Nix: root flake builds each FFI with crane, scoped `cargoExtraArgs = "-p <pkg>"` (keeps RISC Zero guest builds out), copies the header to `$out/include`, and on Darwin fixes the dylib install-name. Each module consumes it with
```nix
externalLibInputs = { stablecoin_ffi = { input = self; packages.default = "stablecoin_ffi"; }; };
```

### 1.4 C++ unit tests with FFI mock (`modules/stablecoin/tests`)

Wired by `mkLogosModule { tests = { dir = ./modules/stablecoin/tests; mockCLibs = [ "stablecoin_ffi" ]; }; }`, output `unit-tests` (aliased `stablecoin-module-tests`).

`tests/CMakeLists.txt`:
```cmake
include(LogosTest)
logos_test(NAME stablecoin_module_tests
    MODULE_SOURCES ../src/stablecoin_module_impl.cpp ../src/stablecoin_module_support.cpp
    TEST_SOURCES main.cpp stablecoin_module_impl_test.cpp stablecoin_module_support_test.cpp
    MOCK_C_SOURCES mocks/mock_stablecoin_ffi.cpp
    GENERATED_SOURCES ../generated_code/lez_core_api.cpp
    EXTRA_INCLUDES ../src ../ffi/include ../generated_code)
```
- `main.cpp`: `#include <logos_test.h>` + `LOGOS_TEST_MAIN()`.
- `mocks/mock_stablecoin_ffi.cpp`: each `extern "C"` FFI symbol calls `LOGOS_CMOCK_RECORD(name)` and returns a malloc'd copy of `LOGOS_CMOCK_RETURN_STRING(name)`.
- `tests/logos_sdk.h`: hand-written adapter `UniversalLezCore` (std-typed) over the Qt test client `LezCore`, plus `struct LogosModules { UniversalLezCore lez_core; }`. Needed because the universal module's generated `modules()` is std-typed but the test framework's dependency client is Qt-typed.
- Test body:
```cpp
LOGOS_TEST(stability_fee_accumulator_reads_once_and_returns_exact_snapshot) {
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    module._logosCoreSetLogosModulesPtr_(&modules);
    context.mockCFunction("stablecoin_program_info").returns(successEnvelope(programInfoValue()));
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());
    const LogosMap response = module.stabilityFeeAccumulator();
    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_TRUE(context.moduleCalledWith("lez_core", "get_account_public",
        QVariantList{QVariant(QString::fromStdString(ID_HEX))}));
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_decode_stability_fee_accumulator"), 1);
}
```
Note: the adapter in `tests/logos_sdk.h` hard-codes the 4-arg `send_generic_public_transaction`. It must match whichever `lez_core` we pin.

### 1.5 ui_qml app (`apps/amm`)

`apps/amm/metadata.json`:
```json
{ "name": "amm_ui", "display_name": "Logos DEX App", "version": "0.3.0", "type": "ui_qml",
  "category": "amm", "main": "amm_ui_plugin", "view": "qml/Main.qml", "icon": "icons/amm.png",
  "dependencies": ["lez_core", "amm_module"],
  "nix": { "packages": { "build": ["pkg-config"],
           "runtime": ["qt6.qtdeclarative", "zstd", "krb5", "abseil-cpp", "libbase58"] }, ... } }
```
No `"interface"` field means the classic model: the plugin (`AmmUiPlugin` + `AmmUiPluginInterface`, IID `org.logos.AmmUiPluginInterface`, `Q_PLUGIN_METADATA(... FILE "../metadata.json")`, deriving the generated `AmmUiBackendViewPluginBase`) is hand-written. `initLogos(LogosAPI*)` creates `AmmUiBackend(api)` and calls `setBackend(m_backend)`.

`CMakeLists.txt`:
```cmake
find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui Network Qml Quick QuickControls2)
include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
add_subdirectory("${LOGOS_WALLET_SOURCE_DIR}" "${CMAKE_CURRENT_BINARY_DIR}/shared-wallet")
logos_module(NAME amm_ui REP_FILE src/AmmUiBackend.rep
    SOURCES src/AmmUiPluginInterface.h src/AmmUiPlugin.{h,cpp} src/AmmUiBackend.{h,cpp} src/RegistryLoader.{h,cpp}
    FIND_PACKAGES Qt6Gui  LINK_LIBRARIES Qt6::Gui Qt6::Network  LINK_TARGETS logos_wallet_access)
```
`.rep` (QtRO view contract): PROPs `isWalletOpen`, `walletStateReady`, `walletExists`, `configPath`, `storagePath`, `walletHome`, `lastSyncedBlock`, `currentBlockHeight`, `sequencerAddr`, `sequencerReachable`, `registryRevision`, `registryUrl`, `activeNetwork`, `networks`; SLOTs for wallet lifecycle (`createAccountPublic`, `createNewDefault(password)` returning a mnemonic, `openExisting`, `disconnectWallet`, `refreshBalances`, ...) and domain ops returning `QVariantMap {status,error,...}`. Amounts cross QtRO as decimal `QString`. The account list is NOT in the .rep; it is a `Q_PROPERTY(WalletAccountModel* accountModel ...)` reached by `logos.model("amm_ui","accountModel")`.

Backend (`AmmUiBackend : AmmUiBackendSimpleSource`) owns `std::make_unique<LogosModules>(m_logosAPI)` and forwards every domain slot to `m_logos->amm_module.<method>(...)` (Qt-typed wrappers). Wallet lifecycle goes through `LogosWalletProvider` (shared lib) over `m_logos->lez_core.*`.

QML entry (`qml/Main.qml`):
```qml
import Logos.Theme
readonly property var backend: logos.module("amm_ui")
readonly property var accountModel: logos.model("amm_ui", "accountModel")
Connections { target: logos
  function onViewModuleReadyChanged(moduleName, isReady) { if (moduleName === "amm_ui") root.ready = isReady && root.backend !== null } }
Component.onCompleted: root.ready = root.backend !== null && logos.isViewModuleReady("amm_ui")
// slot calls: logos.watch(root.backend.swapExactInQuote(a, b, amt, bps), onOk, onErr)
```
Mock-friendly QML: pages receive `backend` and a `runtime` (object with `watch(value, ok, fail)`) as properties so `tst_*.qml` can inject `QtObject` fakes.

Runtime config (env on the Basecamp/standalone process): `AMM_PROGRAM_BIN`, `TOKENS_CONFIG`, `AMM_POOLS_CONFIG`, `AMM_REGISTRY_URL`, `CUSTOM_TOKEN_CONFIG`, `LEE_WALLET_HOME_DIR` (default `~/.lee/wallet`).

### 1.6 Shared wallet (`apps/shared/wallet`)

- `logos_wallet_access` (STATIC lib, Qt Core + Network): `WalletProvider` interface (`connect`, `createWallet`, `snapshot`, `createAccount`, `readPublicAccount`, `submitPublicTransaction(WalletTransaction{programId, accountIds, signingRequirements, instruction (QVector<quint32>)})`, `disconnect`); `LogosWalletProvider` implements it over `lez_core` (`open`, `create_new`, `create_account_public/private`, `get_account_public`, `send_generic_public_transaction`, `get_current_block_height`, `sync_to_block`, `get_last_synced_block`, `get_sequencer_addr`, `list_accounts`, `get_balance`, `save`). In Basecamp it ADOPTS the already-open shared wallet (single `lez_core` instance shared by all apps).
- `Logos.Wallet` QML module (`qt_add_qml_module`, URI `Logos.Wallet`): public types
  - `WalletControl { wallet; accountModel; watchCall: function(r, ok, fail){ logos.watch(r, ok, fail) }; compact; viewportWidth; selectedIndex }` exposes `connected`, `selectedAddress`, `selectedIsPublic`.
  - `TransactionConfirmationDialog` (Popup): `title`, `confirmText`, `cancelText`, `busy`, `summary: Component` (gets `property var snapshot`), `openWithSnapshot(obj)`, signals `confirmed(var snapshot)`, `canceled`.
  - `SubmittedTransaction { title; transactionId }`.
  - `ProgramAccountSelector { sourceModel; accountType; stateField; stateValue; selectionMode: Input|Output; selectedAccountId; createNewSelected }`, signal `selectionChanged(accountId, createNew)`.
- Packaging trick (root flake `postInstall`): copy the built `Logos/Wallet` QML module to `$out/lib/Logos/Wallet` (plugin ROOT, not `qml/`) and strip `linktarget|optional plugin|plugin|classname|typeinfo|prefer` lines from its `qmldir`, so it loads as pure QML inside Basecamp's sandbox (which rejects a qmldir declaring a native plugin, and fails `prefer :/qt/qml/...` with "Invalid null URL"). Standalone needs `QML_IMPORT_PATH=<module>/lib` exported by a wrapper script.
- Tests: `logos_wallet_access_test` (QtTest, `FakeWalletProvider`, fixture `logos_sdk.h`); `logos_wallet_qml_test` (`QUICK_TEST_MAIN(logos_wallet_qml)`, env `QT_QPA_PLATFORM=offscreen;QT_QUICK_BACKEND=software;QML2_IMPORT_PATH=...`).

### 1.7 Tests in apps/amm

- QML unit tests `tests/qml/tst_*.qml` (`import QtTest`, `TestCase`, `import "../../qml/pages" as Pages`, fake backend `QtObject` + fake runtime `watch`). Run per `VALIDATION.md` with `qmltestrunner -import <qtdeclarative>/lib/qt-6/qml -import <logos-design-system>/lib -import <amm>/lib/qml -input apps/amm/tests/qml` under `QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software`. Not wired into CI (`.github/workflows/ci.yml` runs only Rust fmt/clippy/tests, guest build, IDL check).
- UI E2E `tests/*.mjs` via logos-qt-mcp: `const { test, run } = await import(resolve(LOGOS_QT_MCP || "../result-mcp", "test-framework/framework.mjs"))`; locate by `objectName` (`app.findByProperty("objectName", ...)`), `app.inspector.send("click", {objectId})`, `app.getProperties(id)`, `app.waitFor(fn, {timeout, interval, description})`, `app.screenshot()`; verify on-chain with `spel`. Isolated wallet bootstrapped by `tests/testnet/setup-amm-testnet.sh` (fixed mnemonic, `TEST_SEQUENCER_ADDR=http://127.0.0.1:3040`).
- Basecamp-hosted E2E (`apps/token/tests/run-basecamp-e2e.sh`): builds `github:logos-co/logos-basecamp#bin-bundle-dir-inspector`, the wallet module `#install-portable`, the token module `.#install-portable`, the UI `.#token-ui-install-portable`; copies `install/modules/.` into `<run>/user/modules` and `plugins/.` into `<run>/user/plugins`; launches `LogosBasecamp --user-dir <run>/user -platform offscreen` with `QML_INSPECTOR_PORT`, `QML_DISABLE_DISK_CACHE=1`, `LEE_WALLET_HOME_DIR`; waits for the TCP port; runs the `.mjs`. This is the harness to copy.

### 1.8 Distribution pattern used upstream

The release action builds `.#lgx-portable` at a `module_path`, so a monorepo module is published through thin wrapper repos. Example: `logos-blockchain/logos-amm-module/flake.nix`:
```nix
inputs.lez_programs.url = "github:logos-blockchain/lez-programs?rev=f3dfe819004b29868d8f1c7f73cfdbccd967df3b";
outputs = { flake-utils, lez_programs, ... }: flake-utils.lib.eachDefaultSystem (system: {
  packages = { lgx = lez_programs.packages.${system}.amm-module-lgx;
               lgx-portable = lez_programs.packages.${system}.amm-module-lgx-portable; }; });
```
plus a `metadata.json` copy (cross-checked against the built manifest). Same for `logos-amm-ui-module`. Per-module sub-flakes in lez-programs (`modules/amm/flake.nix` with `lez_programs.url = "path:../.."`) are the other route.

---

## 2. logos-module-builder

### 2.1 `mkLogosModule` / `mkLogosQmlModule` inputs (docs/nix-api.md)
```nix
mkLogosModule {            # core (and legacy type "ui")
  src = ./.; configFile = ./metadata.json;
  flakeInputs = inputs;            # deps resolved by NAME: input name must equal metadata dependency name
  externalLibInputs = { foo = inputs.foo; }  # or { input = self; packages.default = "foo_ffi"; [packages.portable = ...]; systems.<sys> = ...; }
  extraBuildInputs = [ ]; extraNativeBuildInputs = [ ];
  configOverrides = { }; preConfigure = "" /* or { externalLibs }: "" */; postInstall = "";
  logosStandalone = null;
  tests = { dir = ./tests; mockCLibs = [ "foo_ffi" ]; preConfigure = ""; extraBuildInputs = []; extraCmakeFlags = []; };
}
mkLogosQmlModule { same minus tests }   # requires type ui_qml + view; "main" optional (QML-only if absent)
```
Outputs per system: `default`, `lib`, `<name>-lib`, `include` (core), `lgx`, `lgx-portable`, `install`, `install-portable`, `unit-tests` (when `tests`), and for QML modules `apps.default` (logos-standalone-app runner), plus `integration-test` and `test-framework` when `tests/*.mjs` exist (auto-detected in lib/mkLogosQmlModule.nix lines 350-391, using `logos-standalone-app.lib.<sys>.mkPluginTest`). Supported systems: aarch64-darwin, x86_64-darwin, aarch64-linux, x86_64-linux (+ x86_64-windows only when logos-nix is threaded in).

### 2.2 `interface: "universal"` rules
- Core: write only `src/<name>_impl.{h,cpp}` with class `<PascalName>Impl : public LogosModuleContext` (override with `codegen.impl_header` / `codegen.impl_class`, as lez_core does: `LEZCoreModule`). Header must be Qt-free (std types, `LogosMap`/`LogosList`, `nlohmann::json`, `std::vector<uint8_t>` maps to `bstr`). `"legacy"` is refused for a `core` module that declares `main`.
- Generation pipeline: (1) `logos-cpp-generator --header-to-lidl` produces `generated_code/<name>.lidl` (the published contract, also carried in the lgx as `assets/lidl`); (2) `logos-qt-host-generator --lidl ... --backend cdylib` produces `<name>_cdylib_glue.{h,cpp}` (the Qt plugin `logos_host` loads); (3) `logos-cpp-generator --lidl ... --backend cdylib` produces `<name>_module_impl.cpp`, `<name>_types.h`, and events glue.
- A `///` comment above a method becomes its description.
- Typed events: a `logos_events:` section in the impl header (e.g. `void bidPlaced(const std::string& auctionId, const std::string& amount);`), emitted by calling it; consumers subscribe with `modules().<dep>.onBidPlaced(...)` armed in `onContextReady()`.
- Generator gotcha (comment in lez_core header): it parses line by line; keep each method declaration on ONE line or it is silently dropped (pinned SDK; logos-cpp-sdk #59/#91).
- `concurrency: "single"` (default) or `"multi"` + `max_workers`.
- `codegen.consumer_api_style`: `lp` (std-typed `modules()`) is the default for universal core modules; `qt` for ui_qml backends.

### 2.3 Calling another module
- From a core module: `#include "logos_sdk.h"` in the .cpp, then `modules().lez_core.get_account_public(id)` (std types). Every dependency must publish a `.lidl` (refused by name otherwise). The flake input name must match the dependency name (`lez_core.url = ...`). To share one wallet instance between UI and core module, make the UI's module input follow it: `auction_engine_module = { url = "path:../../modules/auction_engine"; inputs.lez_core.follows = "lez_core"; };`.
- `optional_dependencies` (not loaded/bundled, check with `logos::CallError err; ... &err, timeout_ms` and `err.code == "object_unavailable"`), `interface_dependencies`, `modules().dynamic("<name>")` exist.
- Dependency entries may be objects: `{ "name": "lez_core", "version": ">=0.4.3" }` (lez_wallet_ui does this).

### 2.4 QML side
- `logos.module("<ui_name>")` returns the typed QtRO replica of the app's own backend (PROPs bind directly; SLOT results are async via `logos.watch(call, ok, err)`); `logos.model("<ui_name>", "<propName>")` for QAbstractItemModels; `logos.isViewModuleReady(name)` + `onViewModuleReadyChanged`. QML-only apps can call core modules directly with `logos.callModule("mod", "method", [args])` (from the create-qml-module skill), but with a C++ backend the convention is to route through the backend.
- Intents: `provides` / `uses` (objects, ui_qml only; `logos.*`/`basecamp.*` namespaces refused); handled by `onIntentRequested(requestId, intent, params, requesterName)` and `logos.respond(...)`. Probably not needed for the auction app.

### 2.5 Sandbox restrictions on ui_qml (docs + local QmlSandbox, section 5)
"QML modules are sandboxed: no network access, no filesystem access outside the module directory" (skills/create-qml-module.md). No native QML plugins from the app's own dir. The C++ backend runs in a separate `ui-host` process and is not inside the QML sandbox (AMM's backend links Qt6::Network for its registry fetch), but Basecamp's spec says apps "that need network access do so indirectly through Logos Modules".

### 2.6 Two ui_qml backend models (pick one)
- Classic (what lez-programs uses): no `interface` field, hand-written `*Plugin` + `*Interface`, `REP_FILE` in CMake, backend holds `std::make_unique<LogosModules>(api)`.
- Universal (what the builder now recommends; template `templates/ui-qml-backend`, doctest `ui-typed-backend`): metadata `"type":"ui_qml","interface":"universal","codegen":{"rep":"src/<name>.rep"}`, write only `src/<name>.rep` + `src/<name>_backend.{h,cpp}` with `class <Name>Backend : public <RepClass>SimpleSource, public LogosUiPluginContext`; plugin glue is generated; `modules()` is Qt-typed; `onContextReady()` is the place to arm event subscriptions. Template layout puts QML under `src/qml/Main.qml` with `"view": "qml/Main.qml"`.
Recommendation: universal model for less plumbing, but reusing `apps/shared/wallet` (`LogosWalletProvider(LogosModules*)`) needs a check that `modules()` gives you a `LogosModules*`-compatible object. If not, use the classic model exactly as AMM does. Both produce identical lgx layouts.

### 2.7 Packaging rules enforced by tooling
- `icon` must be a 256x256 PNG; required for `ui_qml` (manifest 0.4.0+); staged to `assets/icon.png`. A wrong size fails the build.
- `main`/`view` must name files that exist in the built variant (lgpm validates).
- lgx manifests carry Merkle content hashes; lgpm (basecamp 0.3.x) rejects packages without them ("Missing content hashes in manifest").

---

## 3. `lez_core` public API (signing, submitting, polling, accounts)

There are three incompatible API generations. Choosing the pin is the main decision for this project.

| Generation | Rev | metadata version | LEZ (wallet-ffi) | Who uses it |
|---|---|---|---|---|
| A "0.4.1-interim" | `acf0cd501b262c4c15969e3735e85318297b85bf` | 0.4.1 | v0.2.4 | lez-programs (all flakes), deployed AMM testnet guests |
| B published | `f3a08c659c3ccd0dad890687f78bd748fcffde09` | 0.4.3 | `v0.3.0-rc1` | `blockchain-modules-release` catalog (`lez_core-v0.4.3`, released 2026-09-29); `lez_wallet_ui` 1.2.0-rc.1 declares `lez_core >=0.4.3` |
| C HEAD | `b89e5d24df5babc2490d2e5d47756fb1f33435dd` | 0.5.0 | 0.3.0 (`411adc8f`) | lez_wallet_ui HEAD declares `>=0.5.0` |

Common to all (std types in the impl header; the Qt wrappers map `std::string` to `QString`, `int64_t` to `int`, `LogosList` to `QVariantList`, `std::vector<uint8_t>` to `QByteArray`/bstr):
```
std::string create_new(config_path, storage_path, statistics_path, password)   -> BIP39 mnemonic or ""
int64_t     open(config_path, storage_path, statistics_path)                   -> 0 (WALLET_FFI_SUCCESS) on success
int64_t     save()
int64_t     restore_storage(mnemonic, password, uint32_t depth)
std::string create_account_public() / create_account_private()                -> account id hex
LogosList   list_accounts()                                                    -> [{ "account_id": hex64, "is_public": bool }]
std::string get_balance(account_id_hex, bool is_public)                        -> decimal string (u128)
std::string get_account_public(account_id_hex) / get_account_private(...)      -> JSON string (shape differs, below) or "" on error
std::string get_public_account_key(hex) / get_private_account_keys(hex)        -> {nullifier_public_key, viewing_public_key, ...}
std::string account_id_to_base58(hex) / account_id_from_base58(b58)
int64_t     sync_to_block(block) / get_last_synced_block() / get_current_block_height()
std::string get_sequencer_addr()
labels: check_label_available, add_label, resolve_label, get_all_labels_for_account
std::string transfer_public(from_hex, to_hex, amount_le16_hex) (+ shielded/deshielded/private variants)
```
Transaction results are JSON strings: generic tx `{"success": bool, "tx_hash": "<hex>", "secrets": ["<hex32>", ...], "error": "<msg or empty>"}`; transfers `{"success", "tx_hash", "error"}`.

Generation A (lez-programs):
```cpp
std::string send_generic_public_transaction(const std::vector<std::string>& account_ids, const std::vector<bool>& signing_requirements, const std::vector<uint8_t>& instruction, const std::string& program_id_hex);
std::string send_generic_private_transaction(const std::vector<std::string>& account_ids, const std::vector<uint32_t>& instruction, const std::vector<uint8_t>& program_elf, const std::vector<std::vector<uint8_t>>& program_dependencies);
std::string send_program_deployment_transaction(const std::vector<uint8_t>& program_elf);
// get_account_public -> {"program_owner": hex64, "balance": hex32 (u128 LE), "nonce": hex32, "data": hex}
// NO poll_transaction_status. Instruction bytes = risc0 u32 words serialized LE.
```
Generations B and C (identical signatures for these calls):
```cpp
std::string send_generic_public_transaction(const std::vector<std::string>& account_ids, const std::vector<bool>& signing_requirements, const std::vector<uint8_t>& instruction, const std::string& program_id_hex, const std::string& payer_account_id_hex, const std::vector<std::string>& shard_program_account_ids_hex);
//   payer: "" = self-pay from first funded signer; shard ids: empty, or one per account ("" = called program's shard)
std::string send_generic_private_transaction(const std::vector<std::string>& account_ids, const std::vector<uint8_t>& instruction, const std::vector<uint8_t>& program_elf, const std::string& program_kind_json, const std::vector<std::vector<uint8_t>>& program_dependencies, const std::vector<std::string>& dependency_kinds_json, const std::vector<std::string>& shard_program_account_ids_hex);
//   program_kind_json = {"kind":"disclosed"|"shadow"|"undisclosed","account_id":hex, ["program_header":{image_id_hex,program_first_segment_hex,immutable}], ["membership_proof":{index,path:[hex]}]}
//   empty program_elf runs the disclosed program at account_id natively
std::string send_program_deployment_transaction(const std::string& header_account_id_hex, const std::vector<std::string>& segment_account_ids_hex, const std::vector<uint8_t>& program_elf, bool immutable, const std::string& payer_account_id_hex);
bool poll_transaction_status(const std::string& tx_hash_hex);   // true when the tx is found (included); false on not-found OR error
// get_account_public -> {"nonce": hex32, "shards": [{"program": hex64, "data": hex}]}   (sharded account model, no balance/program_owner)
// instruction bytes = Borsh-serialized instruction, passed to the FFI untouched
```
The instruction parameter is `std::vector<uint8_t>` because only a byte-string IPC type survives QtRO (a `std::vector<uint32_t>` became opaque `any` and dropped arguments; see `docs/amm-swap-qtro-serialization-bug.md` referenced in lez-programs).

Implication: the account decoding, instruction encoding (risc0 words vs Borsh), fee payer and shard arguments all change between A and B/C, and the SPEL program's `lee_core` / `spel-framework` pins must match the sequencer. lez-programs guests pin `lee_core` tag v0.2.4 and `spel-framework` rev `1ef0500f8fc8ce3ddf95523726ae159db83f744f`.

---

## 4. Packaging and distribution

### 4.1 Builder outputs (per module flake)
```bash
nix build .#lgx            # dev variant (linux-amd64-dev), needs /nix/store at runtime -> use with `nix build .#app` Basecamp
nix build .#lgx-portable   # self-contained (linux-amd64) -> use with AppImage / catalog Basecamp
nix build .#install            # dir with modules/ (core) or plugins/ (ui) ready to copy into --user-dir
nix build .#install-portable
nix build .#unit-tests     # core with `tests`
nix build .#integration-test   # ui_qml with tests/*.mjs (headless via mkPluginTest)
nix build .#test-framework -o result-mcp   # logos-qt-mcp for running .mjs by hand
nix run .                  # ui_qml: logos-standalone-app with deps bundled
```
Never mix dev and portable variants in one Basecamp; portable uses the `LogosBasecamp` data dir, dev uses `LogosBasecampDev`.

In a monorepo with several builders merged into one package set, the bare `lgx`/`install` attrs collide (last write wins); lez-programs re-exposes them as `<name>-lgx`, `<name>-lgx-portable`, `<name>-install`, `<name>-install-portable`, `<name>-tests`.

### 4.2 nix-bundle-lgx
`nix bundle --bundler github:logos-co/nix-bundle-lgx[#portable|#dual] .#lib`. Variant names: `linux-amd64[-dev]`, `linux-arm64[-dev]`, `darwin-arm64[-dev]`, `darwin-amd64[-dev]` (windows is `windows-x86_64`). Reads `metadata.json` from `drv.src` at eval time and patches `name, version, description, author, type, category, dependencies, view` into the manifest. `passthru.lgxAssets` places files under `assets/` (e.g. `lidl`). The builder already wraps this; you normally do not call it directly.

### 4.3 logos-standalone-app
`logos-standalone --plugin <dir> --modules-dir <dir> --load <module>`; `DEV_QML_PATH=$PWD/src/qml` hot-reloads the view (watched recursively; `LOGOS_QML_HOT_RELOAD=0` to disable). Bundles logos-design-system, so `import Logos.Theme/Controls` work standalone. Add `QML_IMPORT_PATH=<module>/lib` via a wrapper if you ship a root-staged QML module like Logos.Wallet.

### 4.4 Catalog publishing
1. Fork `logos-co/logos-modules-release-base`; edit `logos-repo.json` (`schemaVersion`, `name`, `displayName`, `indexUrl: https://github.com/<owner>/<repo>/releases/download/index/index.json`, `trustedSigners`).
2. `./scripts/add-module.sh https://github.com/<owner>/<module-repo>` per module (submodule + generated `release-<module>.yml` calling `_release-module.yml`, which calls `logos-co/logos-modules-release-action/.github/workflows/release.yml@v1` with `module_path: submodules/<repo>`).
3. Run "Release all modules" (or `./scripts/catalog.sh release-all`). The action builds `.#lgx-portable` per variant (darwin-arm64 on macos-latest, linux-amd64, linux-arm64; windows-x86_64 opt-in via `packages.x86_64-windows`), `lgx merge`, `lgx verify`, optional `lgx sign` (`signing_mode: none|inline|external`), publishes `<module>-v<version>`, rebuilds `index.json`. Re-running an unchanged version is a no-op unless `--force`.
4. Point a client at `https://raw.githubusercontent.com/<owner>/<repo>/<branch>/logos-repo.json`.
5. To avoid re-publishing `lez_core`, add `includesUrl` + `includes.json` referencing `https://raw.githubusercontent.com/logos-blockchain/blockchain-modules-release/refs/heads/main/logos-repo.json` with `"packages": [{"name": "lez_core", "version": "<pinned>"}]`.

Because the action builds at `module_path` with `.#lgx-portable`, each published module needs its own flake exposing bare `lgx`/`lgx-portable`: either thin wrapper repos (like `logos-amm-module`, `logos-amm-ui-module`) or per-subdir flakes.

### 4.5 How Basecamp installs from a catalog (local repo)
- Repositories are managed through the `package_downloader` module: `PackageCoordinator::addRepository/removeRepository/setRepositoryEnabled` invoke `package_downloader.{addRepository,...}`; `catalogChanged` triggers `refreshRepositories()` (`listRepositories`) (`src/PackageCoordinator.cpp` ~983-1065).
- After `getInstalledUiPlugins` + `getInstalledPackages`, `tryFetchCatalog` calls `package_downloader.getCatalog` (10 retries x 200 ms), then `buildCatalogIndexes` (keyed by repositoryUrl + name) and `populateAppsModel` (lines 884-981).
- Install: `buildResolverDepsJson(name, repositoryUrl, versionPins)` walks catalog `dependencies` breadth-first, but only within the same repositoryUrl: `if (depRow.isEmpty()) continue;  // not in this repo — leave unpinned` (line ~1222). `collectCatalogRequired` does the same. Then `beforeInstall` is gated by the confirmation dialog, and `package_manager` installs.
- Local file install: `installPluginFromPath` calls `inspectPackageAsync`, opens the install-confirm dialog, `confirmInstall()`, `package_manager.installPluginAsync()`. Core packages go to `<userDir>/modules`, ui packages to `<userDir>/plugins` (Linux dev: `~/.local/share/Logos/LogosBasecampDev/`; portable: `.../LogosBasecamp/`). CLI equivalent: `lgpm --modules-dir "$DIR/modules" install --file core.lgx` and `lgpm --ui-plugins-dir "$DIR/plugins" install --file ui.lgx`.

### 4.6 logos-scaffold `lgs basecamp`
`lgs basecamp setup | modules [--flake REF] | install | launch <alice|bob> [--log-file] | build [--variant lgx|lgx-portable|all] | build-portable | run <module> | develop <module> | paths | doctor | docs`. Contract (`docs/basecamp-module-requirements.md`): `scaffold.toml` at root; a root or immediate-subdir flake exposing `packages.<system>.lgx` (it will not fall back to `lgx-portable`); built with module-builder 0.3.0 for basecamp 0.3.0 (protocol 0.9); deps resolved from `[modules.<name>]`, then Basecamp built-ins, then the source's own `flake.lock` (so declare `lez_core` as a flake input even in the UI flake), then defaults. Sub-flakes with `path:../<sibling>` inputs get `--override-input` automatically. `basecamp setup` evaluates a multi-GB flake. `lgs run`/`localnet` also give a local sequencer + deploy loop for the SPEL program.

---

## 5. Local Basecamp: discovery, loading, sandbox, deps, UI tests

- Discovery: `PackageCoordinator` asks `package_manager` for installed UI plugins and emits `uiPluginsFetched`; `UIPluginManager::onUiPluginsFetched` (src/UIPluginManager.cpp:86-107) keeps rows with `name` and, for `type == "ui_qml"`, a non-empty `view` (other types need `mainFilePath`). Metadata fields used: `name`, `type`, `view`, `main`/`mainFilePath`, `installDir`, `icon`, `dependencies`, `displayName`/`display_name`, `description`, `version`, `category`, `repositoryUrl`, `hashes.root`.
- Load (`UIPluginManager::loadUiModule`, ~144-185): refuses and shows the missing-deps popup if `PackageCoordinator::missingDepsOf(name)` is non-empty; builds `PluginLoadRequest{installDir, qmlViewPath = installDir + view, mainFilePath (if backend), coreDependencies = dependencies}`.
- `PluginLoader` (src/PluginLoader.cpp): `loadCoreDependencies` calls `CoreModuleManager::loadModule(dep)` for each direct dependency, which calls `logos_core_load_module(name, /*with_dependencies=*/true)`, so transitive deps (`auction_engine_module -> lez_core`) are loaded by liblogos. Failure gives "Failed to load core dependencies for <name>". With a backend: registers a per-spawn UUID token with `capability_module.informModuleToken`, spawns `ViewModuleHost` (ui-host process) with a 30 s ready timeout, then looks for `<name>_replica_factory.{dylib,so,dll}` next to the backend. `loadQmlView` creates a `QQuickWidget`, calls `QmlSandbox::configure(engine, installDir, qmlViewPath, <appDir>/../lib)`, async-precompiles, sets context properties `logos` (a `LogosQmlBridge`) and `isActiveTab`.
- `QmlSandbox::configure` (src/restricted/QmlSandbox.cpp): import paths = appLibDir, view dir, installDir, Qt defaults; `DenyAllNAMFactory` (no network, including `file://` through the NAM); `RestrictedUrlInterceptor` allows only local files under installDir, the view dir, `<appLibDir>/Logos/{Theme,Controls,Icons}` and Qt's default QML dirs, plus `qrc:`; blocks http/https and other schemes; blocks any `qmldir` under the app's dirs that contains a `plugin`/`optional plugin` line. So:
  - Bundled QML must be pure QML/JS (no `qt_add_qml_module` native plugin unless you strip it as lez-programs does).
  - Only `Logos.Theme`, `Logos.Controls`, `Logos.Icons` are available from Basecamp; anything else must be inside the package.
  - No `XMLHttpRequest`, no remote images, no reading files outside installDir from QML. Config files, registries and network go through the backend or a core module.
- Regression checks: `nix build .#sandbox-test` (tests/sandbox, evil_app fixture), `.#qml-tests`, `.#smoke-test`, `.#integration-test`.
- UI tests: `tests/ui-tests.mjs` imports `test-framework/framework.mjs` from `LOGOS_QT_MCP` (default `./result-mcp`, built by `nix build .#logos-qt-mcp -o result-mcp`); `node tests/ui-tests.mjs [filter]` against a running app or `--ci <app-binary>` to launch it. Helpers: `app.click(text, {type})`, `app.expectTexts([...])`, `app.waitFor(...)`, `app.findByProperty`, `app.getProperties`, `app.listInteractive`, `app.screenshot`. The inspector (port 3768, `QML_INSPECTOR_PORT`) is compiled only into dev builds and `#bin-bundle-dir-inspector`, NOT release AppImages. `nix/integration-test.nix` runs offscreen with `LOGOS_USER_DIR=$out/app-data`.
- Reuse for our app: copy `apps/token/tests/run-basecamp-e2e.sh` (prebuilt `install-portable` outputs copied into `--user-dir` plus `bin-bundle-dir-inspector`), and give every interactive QML item a stable `objectName`.

---

## 6. Design system

`logos-co/logos-design-system` exists: `Logos.Theme` (`Theme.palette.*`, `Theme.spacing.*` incl. radii, `Theme.typography.*`, dark/light), `Logos.Controls` (designed: `LogosButton`, `LogosCheckbox`, `LogosComboBox`, `LogosIconButton`, `LogosPaginator`, `LogosSearchBar`, `LogosTabBar`/`LogosTabButton`, `LogosTable`/`LogosTableColumn`, `LogosText`, `LogosTextField`; placeholders: `LogosDialog`, `LogosFrame`, `LogosMenu`, `LogosProgressBar`, `LogosSlider`, `LogosSpinner`, `LogosSwitch`, `LogosTextArea`, ...), `Logos.Icons`. `nix run` opens a storybook; `nix run .#tests` runs its QML tests.

Consumption: Basecamp ships it in `<app>/lib/Logos/{Theme,Controls,Icons}` and the sandbox whitelists exactly those three; logos-standalone-app and logos-module-builder pin it as an input. So apps just `import Logos.Theme` / `import Logos.Controls` and do NOT bundle it. `lez_wallet_ui` imports Logos.Theme and Logos.Controls in 21 files each (plus Logos.Icons). `apps/amm` uses Logos.Theme (e.g. `Theme.palette.warning`) and Logos.Controls in a few files but mostly its own `AmmTheme.qml`; the shared `Logos.Wallet` QML uses hard-coded colors and `QtQuick.Controls.Basic`. To follow reviewers, use Logos.Controls/Theme tokens throughout and avoid an app-specific theme file. For qmltestrunner, add `-import $(nix build github:logos-co/logos-design-system --print-out-paths)/lib`.

---

## 7. Gotchas (Windows 11 + WSL, build times, platforms)

- Build only inside WSL (Ubuntu-20.04 on this box, nix 2.34.8, flakes enabled, 8 cores, about 7 GB RAM, 215 GB free). No native Windows build: the `windows-x86_64` variant is a mingw cross-build from Linux and needs `packages.x86_64-windows`, which lez_core/wallet-ffi do not provide. Keep the repo in the WSL filesystem (not `/mnt/c`) for nix and cargo speed.
- About 7 GB RAM is tight: `lgs basecamp setup` and evaluating the basecamp flake "need several GB of RAM". Raise the WSL memory limit in `.wslconfig` and add swap. Prefer the prebuilt AppImage / `bin-bundle-dir-inspector` over building Basecamp locally.
- Launching Basecamp under WSLg: `setsid nohup env XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland ./LogosBasecamp...` (otherwise SIGABRT, exit 134). Release AppImages have no inspector; for automated tests run offscreen (`-platform offscreen`, `QT_QPA_PLATFORM=offscreen`) with `#bin-bundle-dir-inspector`. Logs: `~/.local/share/Logos/LogosBasecamp/logs/basecamp.log`.
- Driving WSL from Git Bash: `MSYS_NO_PATHCONV=1 wsl.exe -d Ubuntu-20.04 -- bash /mnt/c/.../script.sh | tr -d '\0'`.
- Caches: add `extra-substituters = https://cache.nix.logos.co/public` (key `public:l4HrXgL4nw246+LBh2SOJyhz64BoGegOYLheT/iIAPU=`) and `https://logos-co.cachix.org` (key `logos-co.cachix.org-1:12K8609ho1pCt0erUQrOrs/KmOuhWq/EhVwhzQRb35E=`) via `nixConfig` and `--accept-flake-config` (or trusted-substituters in `/etc/nix/nix.conf`). Without a cache hit, `lez_core` compiles the whole LEZ wallet-ffi + RISC Zero stack (the lez_core lgx is about 217 MB): budget 30-90+ minutes on first build. Building wallet-ffi from source fails on Apple Silicon (ring asm vs cc-wrapper), so macOS relies on the cache.
- RISC Zero guest builds (`make build-programs`) use Docker (`scripts/build-guests.Dockerfile`); Docker Desktop is installed with WSL integration. Use `RISC0_DEV_MODE=1` for tests. The release profile (`debug = 0`, `strip = "symbols"`) is part of program identity: every rebuild changes the ImageID and all PDAs.
- Darwin dylib staging: the builder does not stage external dylibs beside the plugin; lez-programs fixes the install-name in the FFI derivation and exports `DYLD_FALLBACK_LIBRARY_PATH` for standalone. On Linux the rpath into `/nix/store` works for dev; portable relocates.
- Protocol/SDK version skew: a module must be built with the module-builder rev whose `logos-protocol`/`logos-cpp-sdk` match the target Basecamp. Today local Basecamp locks logos-protocol `d5d58e7e`, the builder HEAD locks `8bbc027c`. Scaffold states "module-builder 0.3.0 locks exactly the revs basecamp 0.3.0 does; 0.3.1 is already ahead". Pin `logos-module-builder` to a rev and test against that exact Basecamp release.
- The lez-programs README notes `nix run github:logos-co/logos-basecamp` fails (default app named `logos-basecamp` but binary is `LogosBasecamp`): build then run `result/bin/LogosBasecamp`.

---

## 8. Proposed skeleton

Repository `auction-engine/` (standalone, mirrors lez-programs):

```
auction-engine/
├── Cargo.toml                       # workspace: programs/auction{,/core,/methods}, modules/auction_engine/ffi, tools/idl-gen
│                                    # exclude programs/auction/methods/guest
├── rust-toolchain.toml  rustfmt.toml  clippy.toml  Makefile  cliff.toml
├── flake.nix                        # crane -> auction_ffi; mkLogosModule -> auction-engine-module*;
│                                    # mkLogosQmlModule -> auctions-ui*; aliases *-lgx, *-lgx-portable, *-install*, *-tests
├── artifacts/auction-idl.json
├── programs/auction/
│   ├── core/src/{lib.rs, state.rs, math.rs, pda.rs}   # Instruction enum (borsh), Auction/Bid/Vault accounts, PDA seeds
│   ├── src/{lib.rs, create_auction.rs, place_bid.rs, settle.rs, cancel.rs, liquidate.rs, checks.rs, tests.rs}
│   ├── methods/{Cargo.toml, build.rs, src/lib.rs}
│   └── methods/guest/{Cargo.toml (release profile: debug=0, strip), src/bin/auction.rs}  # #[lez_program(instruction = "auction_core::Instruction")]
├── programs/integration_tests/tests/auction.rs
├── modules/auction_engine/
│   ├── metadata.json                # see below
│   ├── flake.nix                    # standalone build for release CI: inputs logos-module-builder, lez_core (pinned), auction_engine.url = "path:../.."
│   ├── CMakeLists.txt               # logos_module(NAME ${MODULE_NAME} SOURCES src/auction_engine_module_impl.{h,cpp} EXTERNAL_LIBS auction_ffi)
│   ├── README.md                    # headless logoscore runbook
│   ├── ffi/
│   │   ├── Cargo.toml (cdylib+rlib, cbindgen build-dep)  build.rs  cbindgen.toml
│   │   ├── include/auction_ffi.h    # committed, generated
│   │   ├── src/{lib.rs, ffi.rs (envelope + catch_unwind), account.rs}
│   │   ├── src/api/{mod.rs, request.rs, program.rs, decode.rs, plan.rs, quote.rs, tests.rs}
│   │   └── tests/public_api.rs
│   ├── src/
│   │   ├── auction_engine_module_impl.h   # class AuctionEngineModuleImpl : public LogosModuleContext (one-line decls)
│   │   ├── auction_engine_module_impl.cpp # #include "logos_sdk.h"; modules().lez_core.*
│   │   └── auction_support.{h,cpp}        # pure helpers (hex, LE bytes, json parsing) - unit-testable
│   └── tests/
│       ├── CMakeLists.txt (logos_test(...), GENERATED_SOURCES ../generated_code/lez_core_api.cpp)
│       ├── main.cpp (LOGOS_TEST_MAIN())
│       ├── logos_sdk.h                     # UniversalLezCore adapter matching the pinned lez_core signatures
│       ├── mocks/mock_auction_ffi.cpp      # LOGOS_CMOCK_* per exported fn + auction_free
│       ├── auction_engine_module_impl_test.cpp
│       └── auction_support_test.cpp
├── apps/auctions/
│   ├── metadata.json
│   ├── flake.nix                    # mkLogosQmlModule; inputs lez_core, auction_engine_module (path:../../modules/auction_engine, inputs.lez_core.follows), shared_wallet (path, flake=false)
│   ├── CMakeLists.txt               # add_subdirectory(shared wallet); logos_module(NAME auctions REP_FILE src/AuctionsBackend.rep ... LINK_TARGETS logos_wallet_access)
│   ├── icons/auctions.png           # 256x256 PNG (required)
│   ├── src/
│   │   ├── AuctionsBackend.rep
│   │   ├── AuctionsBackend.{h,cpp}  # forwards to modules().auction_engine_module; wallet via LogosWalletProvider
│   │   ├── AuctionsPlugin.{h,cpp}  AuctionsPluginInterface.h   # only if using the classic model
│   │   └── AuctionListModel.{h,cpp} # optional Q_PROPERTY model -> logos.model("auctions","auctionModel")
│   ├── qml/
│   │   ├── Main.qml                 # logos.module("auctions"), readiness, Logos.Theme
│   │   ├── chrome/NavBar.qml        # Logos.Controls LogosTabBar + Wallet.WalletControl
│   │   ├── pages/{AuctionsPage,AuctionDetailPage,CreateAuctionPage,MyBidsPage,LiquidationsPage}.qml
│   │   ├── components/{AuctionCard,BidForm,PriceCurve,CountdownLabel,BidConfirmationSummary}.qml
│   │   └── state/AuctionStore.qml
│   └── tests/
│       ├── qml/tst_{AuctionsPage,BidForm,CreateAuctionPage}.qml   # fake backend + fake runtime.watch
│       ├── create-auction.mjs  place-bid.mjs  settle.mjs          # logos-qt-mcp E2E (auto -> .#integration-test)
│       ├── run-basecamp-e2e.sh                                    # copy of apps/token/tests/run-basecamp-e2e.sh
│       └── testnet/setup-auction-testnet.sh                       # deterministic wallet + deploy + seed
├── apps/shared/wallet/              # vendored copy of lez-programs apps/shared/wallet (Logos.Wallet + logos_wallet_access)
└── .github/workflows/ci.yml         # fmt, clippy, cargo test (RISC0_DEV_MODE=1), IDL check, nix build unit-tests / integration-test
```

`modules/auction_engine/metadata.json`:
```json
{
  "name": "auction_engine_module",
  "display_name": "Auction Engine",
  "version": "0.1.0",
  "type": "core",
  "interface": "universal",
  "category": "defi",
  "description": "RFP-014 liquidation/auction engine: auction discovery, quotes and bid/settle transactions on LEZ.",
  "author": "<you>",
  "main": "auction_engine_module_plugin",
  "dependencies": [{ "name": "lez_core", "version": ">=0.4.3" }],
  "nix": { "packages": { "build": [], "runtime": [] },
           "external_libraries": [ { "name": "auction_ffi" } ],
           "cmake": { "find_packages": [], "extra_sources": [], "extra_include_dirs": [], "extra_link_libraries": [] } }
}
```
(Default impl header is `src/auction_engine_module_impl.h`, class `AuctionEngineModuleImpl`. Use a plain string `"lez_core"` if a version object is not accepted by the pinned builder.)

Impl header outline (each declaration on one line):
```cpp
class AuctionEngineModuleImpl : public LogosModuleContext {
public:
    /// Program ids and singleton PDAs. Configure AUCTION_PROGRAM_ID or AUCTION_PROGRAM_BIN.
    LogosMap programInfo();
    LogosMap setProgramId(const LogosMap& request);
    LogosMap listAuctions(const LogosMap& filter);
    LogosMap getAuction(const std::string& auction_id);
    LogosMap quoteBid(const std::string& auction_id, const nlohmann::json& amount);
    LogosMap createAuction(const LogosMap& request);   // -> {status, error, transactionId, auctionId}
    LogosMap placeBid(const LogosMap& request);        // -> {status, error, transactionId}
    LogosMap settleAuction(const LogosMap& request);
    LogosMap cancelAuction(const LogosMap& request);
    LogosMap transactionStatus(const std::string& tx_hash);  // wraps lez_core.poll_transaction_status (B/C only)
logos_events:
    void auctionUpdated(const std::string& auction_id);
};
```

`apps/auctions/metadata.json` (universal UI model shown; for the classic model drop `interface`/`codegen`):
```json
{
  "name": "auctions",
  "display_name": "Auctions",
  "version": "0.1.0",
  "type": "ui_qml",
  "interface": "universal",
  "category": "defi",
  "description": "Browse, bid on and settle liquidation auctions on the Logos Execution Zone.",
  "author": "<you>",
  "main": "auctions_plugin",
  "view": "qml/Main.qml",
  "icon": "icons/auctions.png",
  "dependencies": ["lez_core", "auction_engine_module"],
  "codegen": { "rep": "src/AuctionsBackend.rep" },
  "nix": { "packages": { "build": ["pkg-config"], "runtime": ["qt6.qtdeclarative"] },
           "external_libraries": [], "cmake": { "find_packages": [], "extra_sources": [], "extra_include_dirs": [], "extra_link_libraries": [] } }
}
```

`.rep` outline:
```
class AuctionsBackend {
    PROP(bool isWalletOpen READONLY)  PROP(bool walletStateReady READONLY)  PROP(bool walletExists READONLY)
    PROP(QString sequencerAddr READONLY)  PROP(bool sequencerReachable READONLY)
    PROP(int currentBlockHeight READONLY)  PROP(int auctionsRevision READONLY)
    SLOT(QString createNewDefault(QString password))  SLOT(bool openExisting())  SLOT(void disconnectWallet())
    SLOT(QString createAccountPublic())  SLOT(void refreshAccounts())
    SLOT(QVariantMap programInfo())  SLOT(QVariantList listAuctions(QVariantMap filter))
    SLOT(QVariantMap getAuction(QString auctionId))  SLOT(QVariantMap quoteBid(QString auctionId, QString amountDecimal))
    SLOT(QVariantMap createAuction(QVariantMap request))  SLOT(QVariantMap placeBid(QVariantMap request))
    SLOT(QVariantMap settleAuction(QVariantMap request))  SLOT(QVariantMap transactionStatus(QString txHash))
}
```

Root `flake.nix` essentials (adapted from lez-programs):
```nix
nixConfig = { extra-substituters = [ "https://cache.nix.logos.co/public" "https://logos-co.cachix.org" ];
              extra-trusted-public-keys = [ "public:l4HrXgL4nw246+LBh2SOJyhz64BoGegOYLheT/iIAPU="
                                            "logos-co.cachix.org-1:12K8609ho1pCt0erUQrOrs/KmOuhWq/EhVwhzQRb35E=" ]; };
inputs = { nixpkgs; flake-utils; crane; rust-overlay;
           logos-module-builder.url = "github:logos-co/logos-module-builder?rev=<pinned>";
           lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=<A|B|C>"; };
auctionFfi = mkHostFfi { package = "auction_ffi"; sourceDir = "modules/auction_engine/ffi"; header = "auction_ffi.h"; };
engine = logos-module-builder.lib.mkLogosModule { src = ./modules/auction_engine; configFile = ./modules/auction_engine/metadata.json;
  flakeInputs = inputs; externalLibInputs.auction_ffi = { input = self; packages.default = "auction_ffi"; };
  tests = { dir = ./modules/auction_engine/tests; mockCLibs = [ "auction_ffi" ]; }; };
ui = logos-module-builder.lib.mkLogosQmlModule { src = ./apps/auctions; configFile = ./apps/auctions/metadata.json;
  flakeInputs = inputs // { auction_engine_module = engine; }; externalLibInputs = { };
  preConfigure = ''cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${./apps/shared/wallet}" "-DLOGOS_WALLET_GENERATED_DIR=$PWD/generated_code/include")'';
  postInstall = <copy + strip Logos/Wallet qmldir to $out/lib/Logos/Wallet, as lez-programs>; };
# expose: auction_ffi, auction-engine-module{,-lgx,-lgx-portable,-install,-install-portable,-tests},
#         auctions-ui{,-lgx,-lgx-portable,-install,-install-portable}, apps.auctions-ui (QML_IMPORT_PATH wrapper)
```

### Commands (run inside WSL)
```bash
# Rust / program
make build-programs                      # docker guest build -> target/guest/auction.bin
RISC0_DEV_MODE=1 cargo test --workspace
cargo test -p auction_ffi && cargo clippy --workspace --all-targets -- -D warnings
make idl                                 # artifacts/auction-idl.json
# Modules
nix build .#auction_ffi -L
nix build .#auction-engine-module -L
nix build .#auction-engine-module-tests -L     # C++ unit tests with FFI + lez_core mocks
nix build .#auctions-ui -L
# QML unit tests
qmltestrunner -import <qtdecl>/lib/qt-6/qml -import <logos-design-system>/lib -import <auctions-ui>/lib -import <auctions-ui>/lib/qml -input apps/auctions/tests/qml
# Standalone run
AUCTION_PROGRAM_BIN=$PWD/target/guest/auction.bin LEE_WALLET_HOME_DIR=... nix run .#auctions-ui
# Packages
nix build .#auction-engine-module-lgx-portable -o result-engine
nix build .#auctions-ui-lgx-portable -o result-ui
nix build 'github:logos-blockchain/logos-execution-zone-module?rev=<pin>#lgx-portable' -o result-core
lgpm --modules-dir "$BC/modules" install --file result-core/*.lgx
lgpm --modules-dir "$BC/modules" install --file result-engine/*.lgx
lgpm --ui-plugins-dir "$BC/plugins" install --file result-ui/*.lgx
# Headless E2E inside Basecamp
nix build .#test-framework -o result-mcp
apps/auctions/tests/run-basecamp-e2e.sh        # bin-bundle-dir-inspector + --user-dir + offscreen + node *.mjs
nix build .#integration-test -L                # standalone-host .mjs run
# Local chain
lgs setup && lgs localnet start && lgs deploy  # or lgs run
# Publish
fork logos-modules-release-base -> add wrapper repos (logos-auction-engine-module, logos-auctions-ui) -> Release all
```

---

## 9. Risks

1. `lez_core` API generation (highest). lez-programs (A, LEZ v0.2.4, 4-arg submit, `{program_owner,balance,nonce,data}`, risc0-word instructions, no `poll_transaction_status`) is incompatible with the published catalog `lez_core` 0.4.3 / HEAD 0.5.0 (B/C, LEZ 0.3.0, 6-arg submit with payer + shard ids, sharded accounts `{nonce, shards}`, Borsh instructions, `poll_transaction_status`). Basecamp runs ONE shared `lez_core`; `lez_wallet_ui` requires `>=0.4.3`. Choose the pin to match the target testnet sequencer, and align `lee_core`/`spel-framework` in the SPEL program, the FFI's instruction encoding, the account decoder, the test adapter `tests/logos_sdk.h`, and `LogosWalletProvider` (which uses the 4-arg form and the old account shape) accordingly. If you pick B/C, the vendored shared wallet must be patched.
2. Builder vs Basecamp protocol skew: pin `logos-module-builder` to the rev matching the Basecamp release reviewers will use (0.3.0 per scaffold docs); unpinned `github:logos-co/logos-module-builder` drifts.
3. Catalog dependency resolution is per repositoryUrl: if our catalog lists only our modules, `lez_core` is left unpinned and must already be installed or available from another enabled repo. Use `includesUrl` to pull `lez_core` from `blockchain-modules-release`, or document the install order.
4. Sandbox: any bundled QML module with a native plugin is rejected; strip `plugin/prefer/classname/typeinfo/linktarget` from bundled qmldirs; keep all network/file I/O in the backend or core module; use only Logos.Theme/Controls/Icons from the host.
5. Release CI shape: needs per-module flakes exposing bare `lgx`/`lgx-portable` (wrapper repos or subdir flakes with `path:../..`); monorepo attr collisions need aliasing.
6. Icon must be exactly 256x256 PNG; `main`/`view` must exist in the variant; lgx needs content hashes.
7. Program identity: release-profile guest builds change the ImageID and all PDAs; ship program id via env/registry, never hard-code a stale one.
8. Header generator drops multi-line method declarations; large integers must travel as decimal strings.
9. WSL: about 7 GB RAM vs multi-GB flake eval; first `lez_core` build without cache can take an hour or more; Basecamp GUI needs the WSLg `XDG_RUNTIME_DIR` override; release AppImages lack the inspector; no native Windows variant for anything depending on `lez_core`.
10. lez-programs QML unit tests and the Basecamp E2E are not in CI there; we need our own CI wiring (`qmltestrunner` derivation or `nix build .#integration-test`) to show reviewers green tests.
