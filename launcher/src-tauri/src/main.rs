// The launcher lives in the tray, so no console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cartridge_launcher_lib::run()
}
