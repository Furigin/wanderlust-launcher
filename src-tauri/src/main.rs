// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// С Этапа 2 лаунчер снова открывает окно Tauri — весь пайплайн установки
// (Этап 1, полностью проверенный консольным прогоном ранее) теперь вызывается
// из Tauri-команды `launch` в lib.rs.
fn main() {
    app_lib::run();
}
