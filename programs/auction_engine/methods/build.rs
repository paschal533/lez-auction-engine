//! Build script that embeds the auction_engine RISC Zero guest ELF.
fn main() {
    risc0_build::embed_methods();
}
