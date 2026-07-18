// Локальные настройки игрока: ник, выбранный пак, RAM, выбор опциональных
// модов. Хранится рядом с остальным лаунчером, а не в реестре/AppData
// нестандартных мест — вся установка живёт в одной папке (см. paths.rs).
use crate::paths::AppPaths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub selected_pack: String,
    #[serde(default = "default_ram_mb")]
    pub ram_mb: u32,
    /// pack_id -> (option_id -> включен ли)
    #[serde(default)]
    pub optional_mods: HashMap<String, HashMap<String, bool>>,
}

fn default_ram_mb() -> u32 {
    2048
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            nickname: String::new(),
            selected_pack: String::new(),
            ram_mb: default_ram_mb(),
            optional_mods: HashMap::new(),
        }
    }
}

pub fn load_settings(paths: &AppPaths) -> Result<Settings> {
    if !paths.launcher_json.is_file() {
        return Ok(Settings::default());
    }
    let text = std::fs::read_to_string(&paths.launcher_json)
        .with_context(|| format!("Не удалось прочитать {}", paths.launcher_json.display()))?;
    // Повреждённый файл настроек не должен блокировать запуск лаунчера —
    // это всего лишь удобство, а не критичные данные игрока.
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

pub fn save_settings(paths: &AppPaths, settings: &Settings) -> Result<()> {
    let text = serde_json::to_string_pretty(settings).context("Не удалось сериализовать настройки")?;
    std::fs::write(&paths.launcher_json, text)
        .with_context(|| format!("Не удалось сохранить {}", paths.launcher_json.display()))
}
