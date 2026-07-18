// Общая логика скачивания: во временный файл, с проверкой хеша, и только
// потом атомарный rename — оборванная докачка не должна оставить игрока
// с файлом, который выглядит установленным, но битым.
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use sha2::Digest;
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub enum HashAlgo {
    Sha256,
    Sha1,
}

const MAX_ATTEMPTS: u32 = 3;

/// Скачивает `url` в `dest`, проверяя хеш содержимого. Если `expected_hash`
/// пуст — хеш не проверяется (используется для maven-сайдкаров, где сам
/// хеш мы и получаем отдельным запросом перед вызовом этой функции).
pub async fn download_and_verify(
    client: &reqwest::Client,
    url: &str,
    algo: HashAlgo,
    expected_hash: &str,
    dest: &Path,
) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Не удалось создать папку {}", parent.display()))?;
    }

    let tmp_path = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));

    let mut last_err = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match try_download(client, url, algo_clone(&algo), expected_hash, &tmp_path).await {
            Ok(()) => {
                // Атомарная замена: если whole-file запись прошла и хеш совпал,
                // только теперь файл становится "видимым" под финальным именем.
                tokio::fs::rename(&tmp_path, dest)
                    .await
                    .with_context(|| format!("Не удалось переименовать {} в {}", tmp_path.display(), dest.display()))?;
                return Ok(());
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                last_err = Some(e);
                if attempt < MAX_ATTEMPTS {
                    let delay_secs = 2u64.pow(attempt - 1); // 1s, 2s, 4s
                    tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                }
            }
        }
    }

    Err(last_err.unwrap()).with_context(|| {
        format!(
            "Не удалось скачать {url} за {MAX_ATTEMPTS} попыток — проверьте интернет-соединение"
        )
    })
}

fn algo_clone(algo: &HashAlgo) -> HashAlgo {
    match algo {
        HashAlgo::Sha256 => HashAlgo::Sha256,
        HashAlgo::Sha1 => HashAlgo::Sha1,
    }
}

async fn try_download(
    client: &reqwest::Client,
    url: &str,
    algo: HashAlgo,
    expected_hash: &str,
    tmp_path: &Path,
) -> Result<()> {
    let response = client
        .get(url)
        .send()
        .await
        .context("Ошибка сети при обращении к серверу")?
        .error_for_status()
        .context("Сервер вернул код ошибки")?;

    let mut file = tokio::fs::File::create(tmp_path)
        .await
        .with_context(|| format!("Не удалось создать временный файл {}", tmp_path.display()))?;

    let mut sha256 = sha2::Sha256::new();
    let mut sha1 = sha1::Sha1::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Обрыв соединения во время скачивания")?;
        match algo {
            HashAlgo::Sha256 => sha256.update(&chunk),
            HashAlgo::Sha1 => sha1.update(&chunk),
        }
        file.write_all(&chunk)
            .await
            .context("Не удалось записать данные на диск (нет места?)")?;
    }
    file.flush().await.context("Не удалось сохранить файл на диск")?;
    drop(file);

    if expected_hash.is_empty() {
        return Ok(());
    }

    let actual_hash = match algo {
        HashAlgo::Sha256 => hex::encode(sha256.finalize()),
        HashAlgo::Sha1 => hex::encode(sha1.finalize()),
    };

    if !actual_hash.eq_ignore_ascii_case(expected_hash) {
        bail!(
            "Контрольная сумма не совпадает (ожидали {expected_hash}, получили {actual_hash}) — файл повреждён при скачивании"
        );
    }

    Ok(())
}
