// Чтение списка опциональных модов из pack.toml/index.toml/*.pw.toml —
// схема подтверждена чтением реальных исходников packwiz-installer
// (PackFile.kt, IndexFile.kt, ModFile.kt), а не документацией (её просто
// нет для этого формата на уровне отдельных полей).
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PackToml {
    index: IndexLoc,
}

#[derive(Debug, Deserialize)]
struct IndexLoc {
    file: String,
}

#[derive(Debug, Deserialize)]
struct IndexToml {
    #[serde(default, rename = "files")]
    files: Vec<IndexFileEntry>,
}

#[derive(Debug, Deserialize)]
struct IndexFileEntry {
    file: String,
    #[serde(default)]
    metafile: bool,
}

#[derive(Debug, Deserialize)]
struct ModToml {
    name: String,
    #[serde(default = "default_side")]
    side: String,
    #[serde(default)]
    option: ModOption,
    #[serde(default)]
    download: ModDownload,
}

#[derive(Debug, Deserialize, Default)]
struct ModDownload {
    url: Option<String>,
}

fn default_side() -> String {
    "both".to_string()
}

#[derive(Debug, Deserialize, Default)]
struct ModOption {
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    description: String,
    #[serde(default, rename = "default")]
    default_value: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OptionalMod {
    /// Путь к .pw.toml относительно корня пака — он же ключ в packwiz.json
    /// (cachedFiles), которым мы сверяемся при сохранении выбора игрока.
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_value: bool,
    /// В самом формате packwiz размера файла нет — узнаём best-effort через
    /// HEAD-запрос по Content-Length. Если сервер его не отдал, просто None.
    pub size_bytes: Option<u64>,
}

/// Список модов с `option.optional = true` клиентской стороны. Не запускает
/// packwiz-installer — это чисто чтение метаданных для экрана настроек.
pub async fn list_optional_mods(packwiz_url: &str) -> Result<Vec<OptionalMod>> {
    let client = reqwest::Client::new();
    let pack_url = reqwest::Url::parse(packwiz_url).context("Некорректный URL pack.toml")?;

    let pack_text = client
        .get(pack_url.clone())
        .send()
        .await
        .context("Не удалось скачать pack.toml")?
        .text()
        .await
        .context("Не удалось прочитать pack.toml")?;
    let pack: PackToml = toml::from_str(&pack_text).context("pack.toml имеет неверный формат")?;

    let index_url = pack_url
        .join(&pack.index.file)
        .context("Не удалось построить URL index.toml")?;
    let index_text = client
        .get(index_url)
        .send()
        .await
        .context("Не удалось скачать index.toml")?
        .text()
        .await
        .context("Не удалось прочитать index.toml")?;
    let index: IndexToml = toml::from_str(&index_text).context("index.toml имеет неверный формат")?;

    let mut result = Vec::new();
    for entry in index.files.iter().filter(|f| f.metafile) {
        let mod_url = match pack_url.join(&entry.file) {
            Ok(u) => u,
            Err(_) => continue, // повреждённая ссылка в паке — пропускаем, не валим весь список
        };
        let mod_text = match client.get(mod_url).send().await {
            Ok(resp) => match resp.text().await {
                Ok(t) => t,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        let Ok(modfile) = toml::from_str::<ModToml>(&mod_text) else {
            continue;
        };
        if !modfile.option.optional {
            continue;
        }
        if modfile.side == "server" {
            continue; // серверные-only моды на клиенте не показываем
        }

        let size_bytes = match &modfile.download.url {
            Some(url) => head_content_length(&client, url).await,
            None => None,
        };

        result.push(OptionalMod {
            id: entry.file.clone(),
            name: modfile.name,
            description: modfile.option.description,
            default_value: modfile.option.default_value,
            size_bytes,
        });
    }

    Ok(result)
}

async fn head_content_length(client: &reqwest::Client, url: &str) -> Option<u64> {
    let resp = client.head(url).send().await.ok()?;
    resp.headers()
        .get(reqwest::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}
