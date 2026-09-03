// Модули лаунчера. paths/manifest/downloader/jre/neoforge/libraries/assets/
// packwiz/launch/auth реализуют Этап 1 (установка+запуск), settings/update —
// локальные настройки и самообновление, остальное — склейка с Tauri-командами
// ниже (Этап 3).
pub mod assets;
pub mod auth;
pub mod cheats;
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
pub mod playtime;
pub mod private_access;
pub mod progress;
pub mod server_status;
pub mod settings;
pub mod system;
pub mod update;
pub mod version;

use tauri::Emitter;

/// Windows CREATE_NO_WINDOW. java.exe — консольное приложение, и без этого
/// флага GUI-лаунчер, порождая его, заставляет Windows создать окно консоли,
/// которое висит в панели задач всю игровую сессию. Флаг подавляет само
/// окно, при этом stdout/stderr по-прежнему перехватываются в лог.
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// В dev-сборке (`cargo tauri dev`) читаем локальный манифест — удобно
// тестировать без публикации. Путь строится от CARGO_MANIFEST_DIR (src-tauri/),
// а не от CWD процесса, чтобы работало одинаково откуда бы ни запустили.
// В релизной сборке (`cargo tauri build`) — боевой манифест с GitHub Pages,
// который и видят игроки.
#[cfg(debug_assertions)]
pub const MANIFEST_SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../dev/manifest.dev.json");
#[cfg(not(debug_assertions))]
pub const MANIFEST_SOURCE: &str = "https://wanderlust-launcher.ruslanyik8.workers.dev/manifest.json";

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

/// Сколько в машине памяти, сколько разумно отдать игре и где потолок.
/// Фронт по этим числам подставляет значение по умолчанию и предупреждает,
/// если игрок выкрутил больше, чем физически есть.
#[tauri::command]
fn get_system_info() -> serde_json::Value {
    match system::total_ram_mb() {
        Some(total) => serde_json::json!({
            "total_ram_mb": total,
            "recommended_ram_mb": system::recommended_ram_mb(total),
            "safe_max_ram_mb": system::safe_max_ram_mb(total),
        }),
        // Не смогли определить — фронт просто не покажет подсказки.
        None => serde_json::json!({
            "total_ram_mb": null,
            "recommended_ram_mb": null,
            "safe_max_ram_mb": null,
        }),
    }
}

/// Наигранное время: всего и по выбранной сборке, в секундах.
#[tauri::command]
fn get_playtime(version_id: String) -> serde_json::Value {
    let Ok(paths) = paths::AppPaths::global() else {
        return serde_json::json!({ "total_seconds": 0, "version_seconds": 0, "sessions": 0 });
    };
    let data = playtime::load(&paths);
    serde_json::json!({
        "total_seconds": data.total_seconds(),
        "version_seconds": data.seconds_for(&version_id),
        "sessions": data.sessions.len(),
    })
}

/// Версия лаунчера — показываем в настройках, чтобы игрок мог назвать её
/// при обращении за помощью, не выискивая в свойствах файла.
#[tauri::command]
fn launcher_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Проверяет код доступа и отдаёт сборки закрытого манифеста.
/// Пустой список — код не подошёл. Ошибка — проблемы с сетью.
#[tauri::command]
async fn unlock_private(code: String) -> Result<Vec<manifest::VersionInfo>, String> {
    match private_access::fetch_private_manifest(MANIFEST_SOURCE, &code).await {
        Ok(Some(m)) => {
            log::info!("[private] код принят, сборок: {}", m.versions.len());
            Ok(m.versions)
        }
        Ok(None) => {
            // Намеренно не пишем в лог сам код: лог игрок может кому-то отправить.
            log::info!("[private] код не подошёл");
            Ok(Vec::new())
        }
        Err(e) => Err(format!("{e:#}")),
    }
}

/// Находит версию по id: сначала в публичном манифесте, затем — в закрытом,
/// если игрок вводил код доступа.
///
/// Без этого запуск и переустановка закрытой сборки не работали бы: её нет
/// в публичном манифесте, и `manifest.version(id)` возвращал бы None.
async fn resolve_version(version_id: &str) -> anyhow::Result<manifest::VersionInfo> {
    let public = manifest::load_manifest(MANIFEST_SOURCE).await?;
    if let Some(v) = public.version(version_id) {
        return Ok(v.clone());
    }

    let code = paths::AppPaths::global()
        .ok()
        .and_then(|p| settings::load_settings(&p).ok())
        .map(|s| s.private_code)
        .unwrap_or_default();

    if !code.trim().is_empty() {
        if let Some(m) = private_access::fetch_private_manifest(MANIFEST_SOURCE, &code).await? {
            if let Some(v) = m.version(version_id) {
                return Ok(v.clone());
            }
        }
    }
    anyhow::bail!("Версия '{version_id}' не найдена")
}

#[tauri::command]
async fn get_server_status(host: String, port: u16) -> server_status::ServerStatus {
    server_status::ping(&host, port).await
}

/// Сносит установку сборки, чтобы следующий запуск поставил её заново.
/// Удаляем строго по белому списку того, чем управляет лаунчер: всё
/// остальное в папке — данные игрока (миры, скриншоты, настройки графики,
/// шейдеры), и потерять их из-за кнопки «переустановить» недопустимо.
#[tauri::command]
async fn reinstall_version(version_id: String) -> Result<(), String> {
    // mods/ здесь нет намеренно: её чистим ниже поштучно, чтобы не унести
    // моды, которые игрок положил туда сам.
    const LAUNCHER_OWNED_DIRS: [&str; 7] = [
        "config",
        "kubejs",
        "defaultconfigs",
        "fancymenu_data",
        "versions",
        "libraries",
        "assets",
    ];
    const LAUNCHER_OWNED_FILES: [&str; 2] = ["packwiz.json", "launcher_profiles.json"];

    let ver = resolve_version(&version_id).await.map_err(|e| format!("{e:#}"))?;
    let paths = paths::AppPaths::for_version(&ver.id, ver.java.major).map_err(|e| format!("{e:#}"))?;

    if !paths.game_dir.is_dir() {
        return Ok(()); // ещё ничего не установлено — переустанавливать нечего
    }

    // Из mods/ удаляем только то, что положил туда packwiz. Мод, докинутый
    // игроком вручную, — его файл, и «переустановить» уносить его не должно.
    //
    // Если packwiz.json прочитать не удалось, отличить своё от чужого нечем,
    // а именно ради таких поломок кнопка и существует — тогда чистим папку
    // целиком, как раньше.
    let tracked = packwiz::tracked_files(&paths.game_dir);
    let mods_dir = paths.game_dir.join("mods");
    if mods_dir.is_dir() {
        if tracked.is_empty() {
            log::warn!("packwiz.json не прочитан — чищу mods/ целиком");
            std::fs::remove_dir_all(&mods_dir)
                .map_err(|e| format!("Не удалось удалить {}: {e}", mods_dir.display()))?;
        } else {
            let mut kept = 0usize;
            let entries = std::fs::read_dir(&mods_dir)
                .map_err(|e| format!("Не удалось прочитать {}: {e}", mods_dir.display()))?;
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let rel = format!("mods/{}", entry.file_name().to_string_lossy()).to_lowercase();
                if tracked.contains(&rel) {
                    let _ = std::fs::remove_file(&path);
                } else {
                    kept += 1;
                }
            }
            if kept > 0 {
                log::info!("Переустановка: в mods/ оставлено {kept} файлов игрока");
            }
        }
    }

    for dir in LAUNCHER_OWNED_DIRS {
        let path = paths.game_dir.join(dir);
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("Не удалось удалить {}: {e}", path.display()))?;
        }
    }
    for file in LAUNCHER_OWNED_FILES {
        let path = paths.game_dir.join(file);
        if path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }

    log::info!("Установка сборки '{version_id}' сброшена, данные игрока сохранены");
    Ok(())
}

/// Открывает папку установки выбранной версии в проводнике — чтобы игрок
/// мог достать скриншоты, миры или логи, не зная про %APPDATA%.
#[tauri::command]
async fn open_game_folder(app: tauri::AppHandle, version_id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let ver = resolve_version(&version_id).await.map_err(|e| format!("{e:#}"))?;
    let paths = paths::AppPaths::for_version(&ver.id, ver.java.major).map_err(|e| format!("{e:#}"))?;
    paths.ensure_dirs().map_err(|e| format!("{e:#}"))?;
    app.opener()
        .open_path(paths.game_dir.display().to_string(), None::<&str>)
        .map_err(|e| format!("{e}"))
}

/// Открывает папку с логами лаунчера и игры — то, что нужно приложить
/// к сообщению о проблеме.
#[tauri::command]
async fn open_logs_folder(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let paths = paths::AppPaths::global().map_err(|e| format!("{e:#}"))?;
    paths.ensure_dirs().map_err(|e| format!("{e:#}"))?;
    app.opener()
        .open_path(paths.root.display().to_string(), None::<&str>)
        .map_err(|e| format!("{e}"))
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
async fn check_for_update(app: tauri::AppHandle) -> Result<bool, String> {
    let manifest = manifest::load_manifest(MANIFEST_SOURCE)
        .await
        .map_err(|e| format!("{e:#}"))?;

    if !update::is_update_available(&manifest) {
        return Ok(false);
    }

    // Показываем игроку экран обновления: загрузка ~14 МБ занимает время, и
    // без видимого прогресса окно выглядело зависшим — люди закрывали его
    // на середине и оставались на старой версии.
    let version = manifest.launcher.version.clone();
    log::info!("Доступна версия {version}, начинаем самообновление");
    let _ = app.emit("update-started", &version);

    let app_for_progress = app.clone();
    let result = update::download_and_apply(&manifest, move |done, total| {
        let _ = app_for_progress.emit("update-progress", (done, total));
    })
    .await;

    match result {
        Ok(()) => {
            let _ = app.emit("update-ready", &version);
            // Даём фронту мгновение отрисовать «Перезапуск...» перед выходом.
            std::thread::sleep(std::time::Duration::from_millis(400));
            std::process::exit(0)
        }
        Err(e) => {
            log::warn!("Самообновление не удалось, продолжаем на текущей версии: {e:#}");
            let _ = app.emit("update-failed", format!("{e:#}"));
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
    let ver = resolve_version(&version_id).await?;
    if ver.status != "ready" {
        anyhow::bail!("Версия «{}» ещё недоступна для запуска", ver.title);
    }

    // Своя game-папка и JRE у каждой версии — см. paths.rs.
    let paths = paths::AppPaths::for_version(&ver.id, ver.java.major)?;
    paths.ensure_dirs()?;

    // Проверка идёт первым делом, до синка пака и до всех загрузок. Два
    // повода именно здесь: packwiz-installer при синке подчищает папку и
    // может унести улику до того, как мы её увидим, а игроку незачем ждать
    // всю установку, чтобы узнать, что его не пустят. При первой установке
    // папки ещё нет — скан просто вернёт пусто.
    let found = cheats::scan(&paths.game_dir, &manifest.anticheat.blocklist);
    if !found.is_empty() {
        // Подробности — только в лог: игроку показываем обезличенный текст,
        // иначе он поймёт, какой файл переименовать (см. cheats.rs).
        let names: Vec<&str> = found.iter().map(|d| d.file.as_str()).collect();
        log::warn!("Запуск заблокирован для {nick} ({}): {}", ver.id, names.join(", "));
        anyhow::bail!("{}", cheats::block_message(&manifest.anticheat.contact));
    }

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

    // Автоподключение включаем только когда у версии задан адрес сервера.
    let server_address = if ver.server.host.trim().is_empty() {
        None
    } else {
        Some(format!("{}:{}", ver.server.host, ver.server.port))
    };

    let mut child = launch::launch_game(
        &paths,
        &java_exe,
        &merged_version,
        &nick,
        settings.ram_mb,
        server_address.as_deref(),
        &reporter,
    )?;

    // Игровой процесс ждём в отдельном потоке — команда должна вернуться
    // сразу же, чтобы фронт мог свернуть окно, не блокируясь на всей сессии.
    // Здесь же засекается время сессии: момент запуска и момент выхода.
    let app_for_exit = app.clone();
    let started_at = playtime::now_unix();
    let session_paths = paths.clone();
    let session_version = ver.id.clone();
    let session_nick = nick.clone();
    std::thread::spawn(move || {
        let code = child
            .wait()
            .ok()
            .and_then(|status| status.code())
            .unwrap_or(-1);

        let seconds = playtime::now_unix().saturating_sub(started_at);
        if let Err(e) = playtime::record_session(
            &session_paths,
            &session_version,
            &session_nick,
            started_at,
            seconds,
        ) {
            // Статистика — не повод портить игроку выход из игры.
            log::warn!("[playtime] не удалось записать сессию: {e:#}");
        }

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
            // Лог пишем всегда, а не только в debug: сообщения об ошибках
            // отправляют игрока «смотреть launcher.log», и файл обязан там
            // быть. В debug дополнительно дублируем в консоль.
            let mut builder = tauri_plugin_log::Builder::default().level(log::LevelFilter::Info);
            if let Ok(paths) = paths::AppPaths::global() {
                let _ = paths.ensure_dirs();
                builder = builder.target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Folder {
                        path: paths.root.clone(),
                        file_name: Some("launcher".to_string()),
                    },
                ));
            }
            if cfg!(debug_assertions) {
                builder = builder.target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ));
            }
            app.handle().plugin(builder.build())?;
            log::info!("Лаунчер {} запущен", env!("CARGO_PKG_VERSION"));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_manifest,
            get_settings,
            save_settings,
            get_optional_mods,
            get_server_status,
            get_system_info,
            get_playtime,
            launcher_version,
            unlock_private,
            reinstall_version,
            open_game_folder,
            open_logs_folder,
            open_url,
            check_for_update,
            launch,
            frontend_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
