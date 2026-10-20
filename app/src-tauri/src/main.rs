//! Tauri entry point (M3). Logic lives in the library.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cw_trace_app::run();
}
