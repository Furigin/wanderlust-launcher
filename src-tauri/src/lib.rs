// Модули Этапа 1: установка JRE и NeoForge. Пока используются напрямую
// из консольного main.rs; в Этапе 3 станут Tauri-командами.
pub mod assets;
pub mod auth;
pub mod downloader;
pub mod jre;
pub mod launch;
pub mod libraries;
pub mod logger;
pub mod manifest;
pub mod neoforge;
pub mod packwiz;
pub mod paths;
pub mod version;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
