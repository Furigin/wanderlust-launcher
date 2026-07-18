// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Этап 1 (шаги 1-2): консольный прогон без GUI — cargo run должен
// установить JRE и NeoForge с нуля. Экраны и Tauri-команды появятся
// в Этапах 2-3; тогда main() снова начнёт вызывать app_lib::run().
use app_lib::logger::log_line;
use app_lib::manifest::load_manifest;
use app_lib::paths::AppPaths;
use app_lib::{jre, neoforge};

// TODO: заменить на реальный https://<...>.github.io/.../manifest.json
// перед раздачей игрокам. Локальный файл — только для разработки.
// Путь строится от CARGO_MANIFEST_DIR (src-tauri/), а не от CWD процесса,
// чтобы `cargo run` работал одинаково из любой директории запуска.
const MANIFEST_SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../dev/manifest.dev.json");

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

    log_line(&paths, "=== Запуск лаунчера (Этап 1: JRE + NeoForge) ===");

    log_line(&paths, "[1/4] Загрузка манифеста...");
    let manifest = load_manifest(MANIFEST_SOURCE).await?;
    log_line(
        &paths,
        &format!(
            "      Minecraft {}, NeoForge {}, Java {}",
            manifest.minecraft.version, manifest.neoforge.version, manifest.java.major
        ),
    );

    log_line(&paths, "[2/4] Проверка Java Runtime...");
    let java_exe = jre::ensure_jre(&paths, &manifest.java.windows_x64).await?;
    log_line(&paths, &format!("      Java готова: {}", java_exe.display()));

    log_line(&paths, "[3/4] Проверка установки NeoForge...");
    neoforge::ensure_neoforge_installed(&paths, &java_exe, &manifest.neoforge.version).await?;
    log_line(&paths, "      NeoForge установлен.");

    log_line(&paths, "[4/4] Готово. (packwiz-синк и запуск игры — следующие шаги Этапа 1)");
    Ok(())
}
