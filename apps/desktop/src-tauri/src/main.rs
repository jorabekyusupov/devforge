#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    if let Some(code) = devforge_desktop_lib::terminal_mode() {
        std::process::exit(code);
    }
    devforge_desktop_lib::run();
}
