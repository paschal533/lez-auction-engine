{
  description = "Auctions app core module (auction_engine)";

  inputs = {
    # The builder Basecamp master (e09962f5) pins; its design system has the components the app UI needs.
    logos-module-builder.url = "github:logos-co/logos-module-builder/de169fd45d7bdf6e28ce1f151b4c34880523ace9";
    # metadata 0.4.1, built on LEZ v0.2.4; never the 0.5.x module (LEZ v0.3.1, 6-argument send).
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=acf0cd501b262c4c15969e3735e85318297b85bf";
    # The repository root flake supplies the auction_ffi library.
    lez_auction.url = "path:../..";
  };

  outputs = inputs@{ logos-module-builder, lez_auction, ... }:
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      externalLibInputs = {
        auction_ffi = { input = lez_auction; packages.default = "auction_ffi"; };
      };
      tests = {
        dir = ./tests;
        mockCLibs = [ "auction_ffi" ];
        # The mocked library is not built, so its header comes from the repository source.
        preConfigure = ''
          cp ${lez_auction}/crates/auction_ffi/include/auction_ffi.h ./generated_code/
        '';
      };
    };
}
