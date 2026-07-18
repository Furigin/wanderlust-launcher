// Простой логгер в файл + дублирование в консоль. Полноценная ротация
// и подписка на события Tauri появятся в Этапе 3 — сейчас нужен только
// факт, что каждый шаг установки остаётся в launcher.log для поддержки.
use crate::paths::AppPaths;
use std::io::Write;

pub fn log_line(paths: &AppPaths, message: &str) {
    println!("{message}");

    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{timestamp}] {message}\n");

    // Ошибку записи в лог-файл не считаем фатальной для установки —
    // игрок должен получить игру, даже если диск для логов недоступен.
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log_file)
    {
        let _ = file.write_all(line.as_bytes());
    }
}
