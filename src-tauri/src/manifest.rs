// Структура манифеста ровно как в ТЗ проекта: единственный URL зашит в
// лаунчер, всё остальное (версии, ссылки, паки) сервер отдаёт отсюда.
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Manifest {
    pub schema: u32,
    pub minecraft: MinecraftInfo,
    pub neoforge: NeoforgeInfo,
    pub java: JavaInfo,
    #[serde(default)]
    pub packs: Vec<PackInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MinecraftInfo {
    pub version: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NeoforgeInfo {
    pub version: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JavaInfo {
    pub major: u32,
    #[serde(rename = "windows-x64")]
    pub windows_x64: JavaBinary,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JavaBinary {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PackInfo {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    pub packwiz_url: String,
}

/// `source` — https:// URL в проде (единственная зашитая ссылка), либо
/// локальный путь к файлу для разработки/тестов без реального GitHub Pages.
pub async fn load_manifest(source: &str) -> Result<Manifest> {
    let body = if source.starts_with("http://") || source.starts_with("https://") {
        reqwest::get(source)
            .await
            .context("Не удалось скачать манифест — проверьте интернет-соединение")?
            .error_for_status()
            .context("Сервер манифеста вернул ошибку")?
            .text()
            .await
            .context("Не удалось прочитать тело ответа манифеста")?
    } else {
        std::fs::read_to_string(source)
            .with_context(|| format!("Не удалось прочитать локальный манифест {source}"))?
    };

    serde_json::from_str(&body).context("Манифест имеет неверный формат JSON")
}
