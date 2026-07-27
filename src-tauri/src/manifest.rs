// Схема манифеста v2: единственный URL зашит в лаунчер, всё остальное сервер
// отдаёт отсюда. Ключевое отличие от v1 — вместо одного пака список версий
// (`versions`), у каждой свой Minecraft/NeoForge/Java/сервер/packwiz-пак, тема
// оформления и статус. Так лаунчер показывает экран выбора (см. фронт), а
// разные версии Minecraft не делят одну game-папку (см. paths.rs).
// Serialize нужен, чтобы отдавать манифест на фронт через Tauri-команду.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Manifest {
    pub schema: u32,
    pub launcher: LauncherInfo,
    #[serde(default)]
    pub links: LinksInfo,
    /// Лента новостей проекта, общая для всех сборок. Пустая — блок новостей
    /// на главном экране просто не показывается.
    #[serde(default)]
    pub news_feed: Vec<NewsItem>,
    pub versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NewsItem {
    #[serde(default)]
    pub date: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
    /// Необязательная ссылка «подробнее» — открывается во внешнем браузере.
    #[serde(default)]
    pub url: String,
}

impl Manifest {
    /// Ищет версию по её `id` (ключ из манифеста, он же имя game-папки).
    pub fn version(&self, id: &str) -> Option<&VersionInfo> {
        self.versions.iter().find(|v| v.id == id)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LauncherInfo {
    pub version: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

/// Одна версия/сборка в списке выбора. Поля, нужные только для запуска
/// (minecraft/neoforge/java/server/pack), помечены `#[serde(default)]` —
/// у карточки со `status = "soon"` их можно вообще не заполнять, запуск такой
/// версии всё равно заблокирован (см. lib.rs::run_launch_pipeline).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VersionInfo {
    /// Стабильный идентификатор: имя game-папки и ключ выбора. Не меняй его
    /// после релиза — иначе у игроков сборка переустановится с нуля.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    /// Тема карточки на фронте: "orange" | "purple" | "default".
    #[serde(default = "default_theme")]
    pub theme: String,
    /// "ready" — играбельна, "soon" — карточка «Скоро», клик заблокирован.
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub news: String,
    #[serde(default)]
    pub minecraft: MinecraftInfo,
    #[serde(default)]
    pub neoforge: NeoforgeInfo,
    #[serde(default)]
    pub java: JavaInfo,
    #[serde(default)]
    pub server: ServerInfo,
    #[serde(default)]
    pub pack: PackInfo,
}

fn default_theme() -> String {
    "default".to_string()
}

fn default_status() -> String {
    "ready".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct MinecraftInfo {
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct NeoforgeInfo {
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct JavaInfo {
    #[serde(default)]
    pub major: u32,
    #[serde(rename = "windows-x64", default)]
    pub windows_x64: JavaBinary,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct JavaBinary {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ServerInfo {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct LinksInfo {
    #[serde(default)]
    pub donate: String,
    #[serde(default)]
    pub discord: String,
    #[serde(default)]
    pub telegram: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct PackInfo {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
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
