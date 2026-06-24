// Tauri v2 build script: codegen reads tauri.conf.json, embeds the frontend (../ui),
// and wires the capabilities. Pure Rust — no C toolchain (mirrors v4 D-0023).
fn main() {
    tauri_build::build()
}
