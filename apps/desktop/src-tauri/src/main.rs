// Keeps a console window from opening next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    wirecatch_desktop_lib::run()
}
