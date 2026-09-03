// NeoForge installer вообще не трогает assets/ (проверено — папка не
// создаётся installer'ом). Формат индекса подтверждён живым запросом к
// piston-meta: {"objects": {"<path>": {"hash": "<sha1>", "size": N}}}.
use crate::downloader::{download_and_verify, HashAlgo};
use crate::paths::AppPaths;
use crate::progress::ProgressReporter;
use crate::version::MergedVersion;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct AssetIndexFile {
    objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

pub async fn ensure_assets(paths: &AppPaths, version: &MergedVersion, reporter: &ProgressReporter) -> Result<()> {
    let client = reqwest::Client::new();
    reporter.report("assets", "Проверка ассетов", 0, 1);

    let indexes_dir = paths.game_dir.join("assets").join("indexes");
    let index_path = indexes_dir.join(format!("{}.json", version.asset_index.id));

    if !index_path.is_file() {
        download_and_verify(
            &client,
            &version.asset_index.url,
            HashAlgo::Sha1,
            &version.asset_index.sha1,
            &index_path,
        )
        .await
        .context("Не удалось скачать asset index")?;
    }

    let index_text = std::fs::read_to_string(&index_path)
        .with_context(|| format!("Не удалось прочитать {}", index_path.display()))?;
    let index: AssetIndexFile =
        serde_json::from_str(&index_text).context("Asset index имеет неверный формат")?;

    let objects_dir = paths.game_dir.join("assets").join("objects");

    // Объектов обычно тысячи — качаем с ограниченной параллельностью, чтобы
    // не открыть тысячи одновременных соединений и не забыть про ретраи.
    const CONCURRENCY: usize = 16;
    let mut pending = Vec::new();
    for object in index.objects.values() {
        let dest = objects_dir
            .join(&object.hash[0..2])
            .join(&object.hash);
        // Быстрая проверка по размеру: полная перепроверка hash на каждый
        // запуск для тысяч файлов была бы слишком медленной; хеш проверяется
        // полностью в момент самого скачивания (download_and_verify).
        let already_ok = dest.is_file() && dest.metadata().map(|m| m.len() == object.size).unwrap_or(false);
        if !already_ok {
            pending.push((object.hash.clone(), object.size, dest));
        }
    }

    let total = pending.len() as u64;
    let done = Arc::new(AtomicU64::new(0));
    reporter.report("assets", "Загрузка ассетов", 0, total.max(1));

    futures_util::stream::iter(pending.into_iter().map(|(hash, _size, dest)| {
        let client = client.clone();
        let done = done.clone();
        let reporter = reporter.clone();
        async move {
            // Ассетов тысячи. Проверяем перед каждым: уже начатые докачаются,
            // а новые не запустятся, и отмена отработает за секунду.
            crate::cancel::check()?;
            let url = format!(
                "https://resources.download.minecraft.net/{}/{}",
                &hash[0..2],
                hash
            );
            let result = download_and_verify(&client, &url, HashAlgo::Sha1, &hash, &dest)
                .await
                .with_context(|| format!("Не удалось скачать asset-объект {hash}"));
            let completed = done.fetch_add(1, Ordering::Relaxed) + 1;
            reporter.report("assets", "Загрузка ассетов", completed, total);
            result
        }
    }))
    .buffer_unordered(CONCURRENCY)
    .collect::<Vec<Result<()>>>()
    .await
    .into_iter()
    .collect::<Result<Vec<()>>>()?;

    reporter.report("assets", "Ассеты готовы", total, total.max(1));
    Ok(())
}
