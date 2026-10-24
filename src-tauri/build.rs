//! Generates the Tauri context and one permission per app command
//! (`allow-<command>`), so `capabilities/` can grant each window only the
//! commands it uses.

/// Must list exactly the commands of `generate_handler!` in `src/lib.rs`.
const COMMANDS: &[&str] = &[
    "get_share",
    "select_network",
    "renew_share",
    "cancel_share",
    "reveal_received",
    "share_paths",
    "share_clipboard",
    "receive_from_phone",
    "pick_files",
    "check_update",
    "install_update",
    "finish_welcome",
];

fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(error) = tauri_build::try_build(attributes) {
        // Same reporting as `tauri_build::build()`.
        println!("{error:#}");
        std::process::exit(1);
    }
}
