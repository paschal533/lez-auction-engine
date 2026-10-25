{
  description = "Auctions app: Basecamp ui_qml view for the liquidation-auction engine";

  inputs = {
    # The builder Basecamp master (e09962f5) pins; same pin as the auction_engine core module.
    logos-module-builder.url = "github:logos-co/logos-module-builder/de169fd45d7bdf6e28ce1f151b4c34880523ace9";
    # metadata 0.4.1, built on LEZ v0.2.4; never the 0.5.x module (LEZ v0.3.1, 6-argument send).
    # The input name matches the metadata.json `dependencies` entry so the builder resolves it.
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=acf0cd501b262c4c15969e3735e85318297b85bf";
    # The core module the backend talks to (the builder reads its interface to generate
    # modules().auction_engine). It shares this flake's lez_core so one wallet instance is resolved.
    auction_engine = {
      url = "path:../../modules/auction_engine";
      inputs.lez_core.follows = "lez_core";
    };
  };

  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      externalLibInputs = { };
    };
}
