// Установщик NeoForge качает только те библиотеки, что нужны его
// процессорам (проверено на реальной установке: из 115 нужных для
// windows-x64 библиотек на диске после installer.jar оказалось только 28).
// Остальные — обычные ванильные зависимости (log4j и т.п.) — докачиваем
// сами, ровно как это делает официальный лаунчер.
use crate::downloader::{download_and_verify, HashAlgo};
use crate::paths::AppPaths;
use crate::progress::ProgressReporter;
use crate::version::{rules_allow, MergedVersion, RawLibrary};
use anyhow::Result;
use std::path::PathBuf;

fn applicable_libraries(version: &MergedVersion) -> Vec<&RawLibrary> {
    version
        .libraries
        .iter()
        .filter(|lib| rules_allow(&lib.rules))
        .filter(|lib| lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()).is_some())
        .collect()
}

pub async fn ensure_libraries(paths: &AppPaths, version: &MergedVersion, reporter: &ProgressReporter) -> Result<()> {
    let client = reqwest::Client::new();
    let libs = applicable_libraries(version);
    let missing: Vec<_> = libs
        .into_iter()
        .filter(|lib| {
            let artifact = lib.downloads.as_ref().unwrap().artifact.as_ref().unwrap();
            !paths.game_dir.join("libraries").join(&artifact.path).is_file()
        })
        .collect();

    let total = missing.len() as u64;
    reporter.report("install", "Проверка библиотек", 0, total.max(1));

    for (i, lib) in missing.iter().enumerate() {
        let artifact = lib.downloads.as_ref().unwrap().artifact.as_ref().unwrap();
        let dest = paths.game_dir.join("libraries").join(&artifact.path);
        download_and_verify(&client, &artifact.url, HashAlgo::Sha1, &artifact.sha1, &dest)
            .await
            .map_err(|e| e.context(format!("Не удалось скачать библиотеку {}", lib.name)))?;
        reporter.report("install", "Загрузка библиотек", (i + 1) as u64, total);
    }

    Ok(())
}

/// Пути ко всем библиотекам + ванильный client.jar, в порядке для `-cp`.
/// Патченный клиентский jar NeoForge сюда не входит — FML находит его сам
/// во время старта через `-DlibraryDirectory` (подтверждено живым запуском).
pub fn build_classpath(paths: &AppPaths, version: &MergedVersion) -> Vec<PathBuf> {
    let mut classpath: Vec<PathBuf> = applicable_libraries(version)
        .iter()
        .map(|lib| {
            let artifact = lib.downloads.as_ref().unwrap().artifact.as_ref().unwrap();
            paths.game_dir.join("libraries").join(&artifact.path)
        })
        .collect();

    // Ванильный client.jar лежит под id родительской версии (version.id
    // смёрдженной структуры — это id NeoForge, а не ванили). assets.id
    // (например "32") с версией Minecraft не совпадает, поэтому id ванильной
    // версии храним отдельным полем vanilla_id при мёрдже.
    classpath.push(vanilla_client_jar(paths, version));

    classpath
}

fn vanilla_client_jar(paths: &AppPaths, version: &MergedVersion) -> PathBuf {
    paths
        .game_dir
        .join("versions")
        .join(&version.vanilla_id)
        .join(format!("{}.jar", version.vanilla_id))
}
