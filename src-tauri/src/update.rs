// Самообновление лаунчера. Windows не даёт перезаписать запущенный exe
// напрямую, но позволяет его переименовать — стандартный паттерн:
// переименовать себя в сторону, записать новую версию на исходное место,
// запустить её, выйти. Осиротевший *.old.exe подчищаем при следующем старте
// (пока текущий процесс жив, файл всё ещё залочен и удалить его нельзя).
use crate::downloader::{download_and_verify, HashAlgo};
use crate::manifest::Manifest;
use anyhow::{bail, Context, Result};

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn is_update_available(manifest: &Manifest) -> bool {
    !manifest.launcher.version.is_empty() && manifest.launcher.version != CURRENT_VERSION
}

/// Скачивает новую версию, проверяет sha256, подменяет исполняемый файл и
/// запускает его. При успехе процесс должен завершиться сразу после вызова —
/// функция не возвращается в обычном потоке управления кроме как с ошибкой.
pub async fn download_and_apply(manifest: &Manifest) -> Result<()> {
    if manifest.launcher.url.is_empty() || manifest.launcher.sha256.is_empty() {
        bail!("В манифесте не указаны url/sha256 новой версии лаунчера");
    }

    let current_exe = std::env::current_exe().context("Не удалось определить путь к своему exe")?;
    let download_path = current_exe.with_extension("new.exe");

    let client = reqwest::Client::new();
    download_and_verify(
        &client,
        &manifest.launcher.url,
        HashAlgo::Sha256,
        &manifest.launcher.sha256,
        &download_path,
    )
    .await
    .context("Не удалось скачать новую версию лаунчера")?;

    let old_backup = current_exe.with_extension("old.exe");
    // Если с прошлого обновления остался старый .old.exe — не страшно,
    // Windows позволит перезаписать rename'ом только если файла нет;
    // на всякий случай пробуем убрать заранее (best-effort).
    let _ = std::fs::remove_file(&old_backup);

    std::fs::rename(&current_exe, &old_backup)
        .context("Не удалось переименовать текущий exe (файл занят другим процессом?)")?;
    std::fs::rename(&download_path, &current_exe).context("Не удалось установить новую версию на место")?;

    std::process::Command::new(&current_exe)
        .spawn()
        .context("Не удалось запустить обновлённую версию")?;

    Ok(())
}

/// Вызывается при каждом старте: убирает хвост от прошлого самообновления.
/// Если файл всё ещё залочен (редкий гонки случай) — просто пропускаем,
/// уберётся при следующем запуске.
pub fn cleanup_old_binary() {
    let Ok(current_exe) = std::env::current_exe() else { return };
    let old_backup = current_exe.with_extension("old.exe");
    if old_backup.is_file() {
        let _ = std::fs::remove_file(&old_backup);
    }
}
