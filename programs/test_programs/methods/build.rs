//! Build script that embeds the test-only attacker guests (`impostor`, `flawed_engine`).
fn main() {
    risc0_build::embed_methods();
}
