// Самообновление лаунчера. Windows не даёт перезаписать запущенный exe
// напрямую, но позволяет его переименовать — стандартный паттерн:
// переименовать себя в сторону, записать новую версию на исходное место,
// запустить её, выйти. Осиротевший *.old.exe подчищаем при следующем старте
// (пока текущий процесс жив, файл всё ещё залочен и удалить его нельзя).
use crate::downloader::{download_and_verify_with_progress, HashAlgo};
use crate::manifest::Manifest;
use anyhow::{bail, Context, Result};

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Обновляемся только если в манифесте версия СТРОГО НОВЕЕ текущей.
///
/// Раньше здесь стояло простое `!=`, и это давало откат назад: стоило
/// манифесту указывать версию старее собранной (например, во время выпуска
/// новой сборки или при откате манифеста), и лаунчер «обновлялся» вниз,
/// перезапускаясь в старую версию. Поймано на живом прогоне: 0.2.1
/// понизил себя до 0.2.0.
pub fn is_update_available(manifest: &Manifest) -> bool {
    let remote = manifest.launcher.version.trim();
    if remote.is_empty() {
        return false;
    }
    is_newer(remote, CURRENT_VERSION)
}

/// Сравнение версий по числовым частям (1.2.10 новее 1.2.9, что строковое
/// сравнение перепутало бы). Нечисловые хвосты вроде "-beta" отбрасываются:
/// для наших нужд важен только порядок номеров.
fn is_newer(remote: &str, current: &str) -> bool {
    parse_version(remote) > parse_version(current)
}

fn parse_version(v: &str) -> Vec<u64> {
    v.split(['.', '-', '+'])
        .map(|part| {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// Скачивает новую версию, проверяет sha256, подменяет исполняемый файл и
/// запускает его. При успехе процесс должен завершиться сразу после вызова —
/// функция не возвращается в обычном потоке управления кроме как с ошибкой.
///
/// `on_progress(скачано, всего)` вызывается по мере загрузки: обновление
/// весит около 14 МБ, и без видимого прогресса игроки закрывали окно на
/// середине, считая, что лаунчер завис.
pub async fn download_and_apply(
    manifest: &Manifest,
    on_progress: impl Fn(u64, u64) + Send + Sync,
) -> Result<()> {
    if manifest.launcher.url.is_empty() || manifest.launcher.sha256.is_empty() {
        bail!("В манифесте не указаны url/sha256 новой версии лаунчера");
    }

    let current_exe = std::env::current_exe().context("Не удалось определить путь к своему exe")?;
    let download_path = current_exe.with_extension("new.exe");

    let client = reqwest::Client::new();
    download_and_verify_with_progress(
        &client,
        &manifest.launcher.url,
        HashAlgo::Sha256,
        &manifest.launcher.sha256,
        &download_path,
        on_progress,
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

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn updates_only_forward() {
        assert!(is_newer("0.2.1", "0.2.0"), "новее — обновляемся");
        assert!(is_newer("0.3.0", "0.2.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        // Ровно тот случай, что поймали живым прогоном: откат назад запрещён.
        assert!(!is_newer("0.2.0", "0.2.1"), "старее — НЕ откатываемся");
        assert!(!is_newer("0.2.0", "0.2.0"), "та же версия — ничего не делаем");
    }

    #[test]
    fn compares_numerically_not_alphabetically() {
        // Строковое сравнение решило бы, что "0.2.9" > "0.2.10".
        assert!(is_newer("0.2.10", "0.2.9"));
        assert!(!is_newer("0.2.9", "0.2.10"));
    }

    #[test]
    fn tolerates_suffixes_and_short_versions() {
        assert!(is_newer("0.3.0-beta", "0.2.0"));
        assert!(!is_newer("0.2.0-beta", "0.3.0"));
        assert!(is_newer("1.1", "1.0.5"));
    }
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
