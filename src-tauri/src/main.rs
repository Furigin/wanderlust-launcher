// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Этап 1 (полностью, консольный прогон без GUI): cargo run должен с нуля
// поставить JRE, NeoForge, ванильные библиотеки, ассеты, синкнуть моды и
// запустить игру. Экраны и Tauri-команды появятся в Этапах 2-3; тогда
// main() снова начнёт вызывать app_lib::run().
use app_lib::logger::log_line;
use app_lib::manifest::load_manifest;
use app_lib::paths::AppPaths;
use app_lib::{assets, jre, launch, libraries, neoforge, packwiz, version};

// TODO: заменить на реальный https://<...>.github.io/.../manifest.json
// перед раздачей игрокам. Локальный файл — только для разработки.
// Путь строится от CARGO_MANIFEST_DIR (src-tauri/), а не от CWD процесса,
// чтобы `cargo run` работал одинаково из любой директории запуска.
const MANIFEST_SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../dev/manifest.dev.json");

// Тестовый pack.toml (проверенный живым прогоном packwiz-installer) — тоже
// только для разработки, пока нет реальной раздачи модов через GitHub Pages.
// CARGO_MANIFEST_DIR на Windows содержит обратные слэши, а file:// URI их
// не допускает (java.net.URI падает с "Illegal character in path") —
// собираем URL в рантайме, заменяя '\' на '/'.
const DEV_PACKWIZ_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../dev/testpack/pack.toml");

// Ник для консольного теста запуска; в Этапе 2 придёт из поля ввода GUI.
const DEV_TEST_NICK: &str = "LauncherDevTest";

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("Ошибка: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let paths = AppPaths::new()?;
    paths.ensure_dirs()?;

    log_line(&paths, "=== Запуск лаунчера (Этап 1 целиком) ===");

    log_line(&paths, "[1/7] Загрузка манифеста...");
    let manifest = load_manifest(MANIFEST_SOURCE).await?;
    log_line(
        &paths,
        &format!(
            "      Minecraft {}, NeoForge {}, Java {}",
            manifest.minecraft.version, manifest.neoforge.version, manifest.java.major
        ),
    );

    log_line(&paths, "[2/7] Проверка Java Runtime...");
    let java_exe = jre::ensure_jre(&paths, &manifest.java.windows_x64).await?;
    log_line(&paths, &format!("      Java готова: {}", java_exe.display()));

    log_line(&paths, "[3/7] Проверка установки NeoForge...");
    neoforge::ensure_neoforge_installed(&paths, &java_exe, &manifest.neoforge.version).await?;
    log_line(&paths, "      NeoForge установлен.");

    log_line(&paths, "[4/7] Проверка ванильных библиотек...");
    let neoforge_id = format!("neoforge-{}", manifest.neoforge.version);
    let merged_version = version::load_merged_version(&paths.game_dir, &neoforge_id)?;
    libraries::ensure_libraries(&paths, &merged_version).await?;
    log_line(&paths, "      Библиотеки на месте.");

    log_line(&paths, "[5/7] Проверка ассетов...");
    assets::ensure_assets(&paths, &merged_version).await?;
    log_line(&paths, "      Ассеты на месте.");

    log_line(&paths, "[6/7] Синхронизация модов (packwiz)...");
    let dev_packwiz_url = format!("file:///{}", DEV_PACKWIZ_PATH.replace('\\', "/"));
    packwiz::sync_modpack(&paths, &java_exe, &dev_packwiz_url).await?;
    log_line(&paths, "      Моды синхронизированы.");

    log_line(&paths, "[7/7] Запуск игры...");
    let mut child = launch::launch_game(&paths, &java_exe, &merged_version, DEV_TEST_NICK)?;
    log_line(&paths, &format!("      Процесс игры запущен, PID {}", child.id()));

    let status = child.wait()?;
    log_line(&paths, &format!("      Игра завершилась: {status}"));

    Ok(())
}
