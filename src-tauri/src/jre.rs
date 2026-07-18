// Java 25 практически ни у кого не установлена, и ничего не подходит на
// системную JVM: если она есть, но не той версии, игра упадёт на
// "Unsupported class file version" ещё до того как игрок увидит окно.
// Поэтому лаунчер всегда носит с собой свою copy JRE, скачанную с Adoptium.
use crate::downloader::{download_and_verify, HashAlgo};
use crate::manifest::JavaBinary;
use crate::paths::AppPaths;
use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::Path;

/// Возвращает путь к java.exe, скачивая и распаковывая JRE при необходимости.
pub async fn ensure_jre(paths: &AppPaths, java: &JavaBinary) -> Result<std::path::PathBuf> {
    let java_exe = paths.java_exe();
    if java_exe.is_file() {
        return Ok(java_exe);
    }

    std::fs::create_dir_all(&paths.runtime_dir)
        .with_context(|| format!("Не удалось создать папку {}", paths.runtime_dir.display()))?;

    let zip_path = paths.runtime_dir.join("jre-download.zip");
    let client = reqwest::Client::new();

    download_and_verify(&client, &java.url, HashAlgo::Sha256, &java.sha256, &zip_path)
        .await
        .context("Не удалось скачать Java Runtime")?;

    extract_jre_zip(&zip_path, &paths.jre_dir)
        .context("Не удалось распаковать архив Java Runtime")?;

    let _ = std::fs::remove_file(&zip_path);

    if !java_exe.is_file() {
        bail!(
            "После распаковки JRE не найден файл {} — архив имеет неожиданную структуру",
            java_exe.display()
        );
    }

    Ok(java_exe)
}

/// Архивы Adoptium содержат один корневой каталог вида `jdk-25.0.3+9-jre/`.
/// Имя каталога меняется от сборки к сборке, поэтому просто срезаем первый
/// компонент пути у каждой записи и распаковываем прямо в `jre_dir`.
fn extract_jre_zip(zip_path: &Path, jre_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)
        .with_context(|| format!("Не удалось открыть {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("Файл JRE не является корректным zip-архивом")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let entry_path = match entry.enclosed_name() {
            Some(p) => p,
            None => continue, // пропускаем небезопасные пути (../..) внутри архива
        };

        // Срезаем первый компонент (корневую папку jdk-*-jre/).
        let stripped: std::path::PathBuf = entry_path.components().skip(1).collect();
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let out_path = jre_dir.join(stripped);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .with_context(|| format!("Не удалось создать файл {}", out_path.display()))?;
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf)?;
            std::io::Write::write_all(&mut out_file, &buf)?;
        }
    }

    Ok(())
}
