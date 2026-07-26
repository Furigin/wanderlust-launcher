// Модули лаунчера. paths/manifest/downloader/jre/neoforge/libraries/assets/
// packwiz/launch/auth реализуют Этап 1 (установка+запуск), settings/update —
// локальные настройки и самообновление, остальное — склейка с Tauri-командами
// ниже (Этап 3).
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
pub mod packwiz_meta;
pub mod paths;
pub mod progress;
pub mod settings;
pub mod update;
pub mod version;

use anyhow::Context as _;
use tauri::Emitter;

// В dev-сборке (`cargo tauri dev`) читаем локальный манифест — удобно
// тестировать без публикации. Путь строится от CARGO_MANIFEST_DIR (src-tauri/),
// а не от CWD процесса, чтобы работало одинаково откуда бы ни запустили.
// В релизной сборке (`cargo tauri build`) — боевой манифест с GitHub Pages,
// который и видят игроки.
#[cfg(debug_assertions)]
pub const MANIFEST_SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../dev/manifest.dev.json");
#[cfg(not(debug_assertions))]
pub const MANIFEST_SOURCE: &str = "https://furigin.github.io/wanderlust_launcher/manifest.json";

#[tauri::command]
async fn get_manifest() -> Result<manifest::Manifest, String> {
    manifest::load_manifest(MANIFEST_SOURCE).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn get_settings() -> Result<settings::Settings, String> {
    let paths = paths::AppPaths::global().map_err(|e| format!("{e:#}"))?;
    settings::load_settings(&paths).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn save_settings(settings: settings::Settings) -> Result<(), String> {
    let paths = paths::AppPaths::global().map_err(|e| format!("{e:#}"))?;
    paths.ensure_dirs().map_err(|e| format!("{e:#}"))?;
    settings::save_settings(&paths, &settings).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn get_optional_mods(packwiz_url: String) -> Result<Vec<packwiz_meta::OptionalMod>, String> {
    packwiz_meta::list_optional_mods(&packwiz_url)
        .await
        .map_err(|e| format!("{e:#}"))
}

// Единственный способ убедиться, что фронт реально дошёл до вызова Tauri API
// (в этой среде нет возможности визуально увидеть окно) — форвардим сюда
// свои console-ошибки и контрольные точки загрузки; tauri-plugin-log в
// debug-сборке печатает это прямо в терминал, где запущен `cargo run`.
#[tauri::command]
fn frontend_log(level: String, message: String) {
    match level.as_str() {
        "error" => log::error!("[frontend] {message}"),
        "warn" => log::warn!("[frontend] {message}"),
        _ => log::info!("[frontend] {message}"),
    }
}

#[tauri::command]
async fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(url, None::<&str>).map_err(|e| format!("{e}"))
}

/// Сверяет версию лаунчера из манифеста со своей. Если есть новее — качает,
/// подменяет exe и перезапускается (процесс завершается изнутри при успехе).
/// Ошибка самообновления не должна мешать игроку просто поиграть — в этом
/// случае молча остаёмся на текущей версии и продолжаем как обычно.
#[tauri::command]
async fn check_for_update() -> Result<bool, String> {
    let manifest = manifest::load_manifest(MANIFEST_SOURCE)
        .await
        .map_err(|e| format!("{e:#}"))?;

    if !update::is_update_available(&manifest) {
        return Ok(false);
    }

    match update::download_and_apply(&manifest).await {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            log::warn!("Самообновление не удалось, продолжаем на текущей версии: {e:#}");
            Ok(false)
        }
    }
}

#[tauri::command]
async fn launch(app: tauri::AppHandle, version_id: String, nick: String) -> Result<(), String> {
    run_launch_pipeline(app, version_id, nick).await.map_err(|e| format!("{e:#}"))
}

async fn run_launch_pipeline(app: tauri::AppHandle, version_id: String, nick: String) -> anyhow::Result<()> {
    let manifest = manifest::load_manifest(MANIFEST_SOURCE).await?;
    let ver = manifest
        .version(&version_id)
        .with_context(|| format!("Версия '{version_id}' не найдена в манифесте"))?
        .clone();
    if ver.status != "ready" {
        anyhow::bail!("Версия «{}» ещё недоступна для запуска", ver.title);
    }

    // Своя game-папка и JRE у каждой версии — см. paths.rs.
    let paths = paths::AppPaths::for_version(&ver.id, ver.java.major)?;
    paths.ensure_dirs()?;

    let app_for_events = app.clone();
    let reporter = progress::ProgressReporter::new(move |ev| {
        let _ = app_for_events.emit("progress", ev);
    });

    let packwiz_url = ver.pack.packwiz_url.clone();

    let java_exe = jre::ensure_jre(&paths, &ver.java.windows_x64, &reporter).await?;
    neoforge::ensure_neoforge_installed(&paths, &java_exe, &ver.neoforge.version, &reporter).await?;

    let neoforge_id = format!("neoforge-{}", ver.neoforge.version);
    let merged_version = version::load_merged_version(&paths.game_dir, &neoforge_id)?;
    libraries::ensure_libraries(&paths, &merged_version, &reporter).await?;
    assets::ensure_assets(&paths, &merged_version, &reporter).await?;

    packwiz::sync_modpack(&paths, &java_exe, &packwiz_url, &reporter).await?;

    // Сохраняем ник сразу после успешного клика "Играть", и применяем
    // выбор опциональных модов поверх обычного синка — CLI packwiz-installer
    // сам спрашивать про опции не умеет (см. packwiz.rs). Настройки общие для
    // всех версий, но выбор опций берём для текущей (optional_for).
    let mut settings = settings::load_settings(&paths)?;
    settings.nickname = nick.clone();
    settings::save_settings(&paths, &settings)?;

    if packwiz::reconcile_optional_mods(&paths, &settings.optional_for(&ver.id)).await? {
        packwiz::sync_modpack(&paths, &java_exe, &packwiz_url, &reporter).await?;
    }

    let mut child = launch::launch_game(
        &paths,
        &java_exe,
        &merged_version,
        &nick,
        settings.ram_mb,
        &reporter,
    )?;

    // Игровой процесс ждём в отдельном потоке — команда должна вернуться
    // сразу же, чтобы фронт мог свернуть окно, не блокируясь на всей сессии.
    let app_for_exit = app.clone();
    std::thread::spawn(move || {
        let code = child
            .wait()
            .ok()
            .and_then(|status| status.code())
            .unwrap_or(-1);
        let _ = app_for_exit.emit("game-exited", code);
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    update::cleanup_old_binary();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
        .invoke_handler(tauri::generate_handler![
            get_manifest,
            get_settings,
            save_settings,
            get_optional_mods,
            open_url,
            check_for_update,
            launch,
            frontend_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
